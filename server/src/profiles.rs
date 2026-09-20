//! Sibling profile discovery for #107: which isolated deployments share this host, so
//! `onboard` can pick a free port and `gateway install` can refuse to collide with one.
//!
//! Profile names are local deployment metadata only. Nothing here reads another
//! profile's data beyond the `port` line of its config.toml and the data root its
//! service definition names.

use std::{fmt, fs, path::Path};

use crate::{config, onboard, service};

/// Every profile with a data root on this host, `default` first, then by name.
pub fn siblings(home_dir: &Path) -> Vec<config::Profile> {
    let mut found = Vec::new();
    let default_root = config::profile_root(home_dir, config::DEFAULT_PROFILE);
    if default_root.is_dir() {
        found.push(config::Profile {
            name: config::DEFAULT_PROFILE.to_owned(),
            root: default_root,
        });
    }
    let mut named: Vec<String> = fs::read_dir(config::profiles_dir(home_dir))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| config::validate_profile_name(name).is_ok())
                .collect()
        })
        .unwrap_or_default();
    named.sort();
    found.extend(named.into_iter().map(|name| config::Profile {
        root: config::profile_root(home_dir, &name),
        name,
    }));
    found
}

/// The `port` from a profile's config.toml, without loading (and thereby creating) a config.
pub fn configured_port(root: &Path) -> Option<u16> {
    fs::read_to_string(root.join("config.toml"))
        .ok()
        .and_then(|raw| toml::from_str::<toml::Value>(&raw).ok())
        .and_then(|doc| doc.get("port").and_then(toml::Value::as_integer))
        .and_then(|port| u16::try_from(port).ok())
}

/// The first port above the default one that no sibling configured and nothing is
/// listening on. The default port itself is always left to the default profile.
pub fn pick_free_port(taken: &[u16], is_listening: impl Fn(u16) -> bool) -> Option<u16> {
    const CANDIDATES: u16 = 200;
    (onboard::DEFAULT_PORT + 1..=onboard::DEFAULT_PORT + CANDIDATES)
        .find(|port| !taken.contains(port) && !is_listening(*port))
}

/// Why installing `profile` would step on a sibling. Every message names both profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Collision {
    /// Both profiles resolve to the same data root.
    DataRoot { profile: String, other: String },
    /// The sibling's config.toml already claims the port.
    Port {
        profile: String,
        other: String,
        port: u16,
    },
    /// The sibling's installed service already runs this data root.
    Definition { profile: String, other: String },
}

impl fmt::Display for Collision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataRoot { profile, other } => write!(
                f,
                "profile {profile} would use the data root of profile {other}; \
                 run this command with `--profile {other}` instead"
            ),
            Self::Port {
                profile,
                other,
                port,
            } => write!(
                f,
                "profile {profile} and profile {other} are both configured for port {port}; \
                 change `port` in one config.toml (or rerun `nolune onboard --profile {profile} --port <free port>`)"
            ),
            Self::Definition { profile, other } => write!(
                f,
                "the background service of profile {other} already runs the data root of \
                 profile {profile}; remove it with `nolune gateway uninstall --profile {other}` first"
            ),
        }
    }
}

/// Collisions between `target` (about to be installed on `port`) and every sibling profile.
pub fn collisions(home_dir: &Path, target: &config::Profile, port: u16) -> Vec<Collision> {
    let mut found = Vec::new();
    for sibling in siblings(home_dir) {
        if sibling.name == target.name {
            continue;
        }
        if sibling.root == target.root {
            found.push(Collision::DataRoot {
                profile: target.name.clone(),
                other: sibling.name.clone(),
            });
            continue;
        }
        if configured_port(&sibling.root) == Some(port) {
            found.push(Collision::Port {
                profile: target.name.clone(),
                other: sibling.name.clone(),
                port,
            });
        }
        let runs_our_root = fs::read_to_string(service::definition_path(home_dir, &sibling.name))
            .ok()
            .and_then(|contents| service::definition_home(&contents))
            .is_some_and(|home| home == target.root);
        if runs_our_root {
            found.push(Collision::Definition {
                profile: target.name.clone(),
                other: sibling.name.clone(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn home_with(profiles: &[(&str, u16)]) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        for (name, port) in profiles {
            let root = config::profile_root(&home, name);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("config.toml"),
                format!("port = {port}\nauth_token = \"t\"\n"),
            )
            .unwrap();
        }
        (tmp, home)
    }

    fn profile(home: &Path, name: &str) -> config::Profile {
        config::Profile {
            name: name.to_owned(),
            root: config::profile_root(home, name),
        }
    }

    #[test]
    fn siblings_are_the_default_root_and_valid_named_roots() {
        let (_tmp, home) = home_with(&[("default", 26559), ("yuki", 26561), ("molinka", 26560)]);
        fs::create_dir_all(config::profiles_dir(&home).join("Not-Valid")).unwrap();
        fs::write(config::profiles_dir(&home).join("stray-file"), "").unwrap();

        let names: Vec<String> = siblings(&home).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["default", "molinka", "yuki"]);
        let roots: Vec<PathBuf> = siblings(&home).into_iter().map(|p| p.root).collect();
        assert_eq!(
            roots,
            [
                config::profile_root(&home, "default"),
                config::profile_root(&home, "molinka"),
                config::profile_root(&home, "yuki"),
            ]
        );
    }

    #[test]
    fn siblings_is_empty_on_a_fresh_host_and_skips_a_missing_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(siblings(&tmp.path().join("nobody")).is_empty());

        let (_tmp, home) = home_with(&[("molinka", 26560)]);
        let names: Vec<String> = siblings(&home).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["molinka"]);
    }

    #[test]
    fn configured_port_reads_only_the_port_line() {
        let (_tmp, home) = home_with(&[("molinka", 4242)]);
        assert_eq!(
            configured_port(&config::profile_root(&home, "molinka")),
            Some(4242)
        );
        assert_eq!(configured_port(&home.join("missing")), None);
        fs::write(
            config::profile_root(&home, "molinka").join("config.toml"),
            "auth_token = \"t\"\n",
        )
        .unwrap();
        assert_eq!(
            configured_port(&config::profile_root(&home, "molinka")),
            None
        );
        assert!(
            !home.join("missing").exists(),
            "probing a port must not create a workspace"
        );
    }

    #[test]
    fn free_port_skips_the_default_taken_and_listening_ports() {
        let base = onboard::DEFAULT_PORT;
        assert_eq!(pick_free_port(&[], |_| false), Some(base + 1));
        assert_eq!(
            pick_free_port(&[base + 1], |port| port == base + 2),
            Some(base + 3)
        );
        // The default profile owns the default port even before it is onboarded.
        assert_ne!(pick_free_port(&[base + 1], |_| false), Some(base));
        assert_eq!(pick_free_port(&[], |_| true), None);
    }

    #[test]
    fn a_shared_port_is_a_collision_that_names_both_profiles() {
        let (_tmp, home) = home_with(&[("default", 26559), ("molinka", 26560), ("yuki", 26561)]);

        let found = collisions(&home, &profile(&home, "yuki"), 26560);
        assert_eq!(
            found,
            [Collision::Port {
                profile: "yuki".into(),
                other: "molinka".into(),
                port: 26560,
            }]
        );
        let message = found[0].to_string();
        assert!(message.contains("yuki"), "{message}");
        assert!(message.contains("molinka"), "{message}");
        assert!(message.contains("26560"), "{message}");

        // Its own port and an unused one are fine; the sibling with the same name is itself.
        assert!(collisions(&home, &profile(&home, "yuki"), 26561).is_empty());
        assert!(collisions(&home, &profile(&home, "yuki"), 26999).is_empty());
    }

    #[test]
    fn a_shared_data_root_is_a_collision() {
        let (_tmp, home) = home_with(&[("default", 26559), ("molinka", 26560)]);
        // `NOLUNE_HOME=~/.nolune-profiles/molinka nolune gateway install` addresses the
        // default profile but molinka's root.
        let target = config::Profile {
            name: "default".into(),
            root: config::profile_root(&home, "molinka"),
        };

        let found = collisions(&home, &target, 26559);
        assert_eq!(
            found,
            [Collision::DataRoot {
                profile: "default".into(),
                other: "molinka".into(),
            }]
        );
        let message = found[0].to_string();
        assert!(
            message.contains("default") && message.contains("molinka"),
            "{message}"
        );
    }

    #[test]
    fn a_sibling_service_already_running_this_root_is_a_collision() {
        let (_tmp, home) = home_with(&[("default", 26559), ("molinka", 26560)]);
        let molinka = profile(&home, "molinka");
        // The default profile's service was installed with NOLUNE_HOME pointing at molinka.
        let foreign = service::ServiceSpec {
            binary: PathBuf::from("/bin/nolune"),
            home: molinka.root.clone(),
            profile: "default".into(),
        };
        let contents = if cfg!(target_os = "macos") {
            service::render_launchd_plist(&foreign)
        } else {
            service::render_systemd_unit(&foreign)
        };
        service::write_definition(&service::definition_path(&home, "default"), &contents).unwrap();

        let found = collisions(&home, &molinka, 26560);
        assert_eq!(
            found,
            [Collision::Definition {
                profile: "molinka".into(),
                other: "default".into(),
            }]
        );
        let message = found[0].to_string();
        assert!(
            message.contains("default") && message.contains("molinka"),
            "{message}"
        );

        // A sibling whose service runs its own root is not a collision.
        let own = service::ServiceSpec {
            binary: PathBuf::from("/bin/nolune"),
            home: config::profile_root(&home, "default"),
            profile: "default".into(),
        };
        let contents = if cfg!(target_os = "macos") {
            service::render_launchd_plist(&own)
        } else {
            service::render_systemd_unit(&own)
        };
        service::write_definition(&service::definition_path(&home, "default"), &contents).unwrap();
        assert!(collisions(&home, &molinka, 26560).is_empty());
    }
}
