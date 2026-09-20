# Third-party notices

Nolune is MIT-licensed (see [LICENSE](LICENSE)). The components below are
distributed with, or downloaded by, Nolune releases under their own licenses.

## Cua Driver

Computer use on connected machines is driven by Cua Driver, the Rust driver
published from <https://github.com/trycua/cua> by Cua AI, Inc.

Nolune pins one tested release, Cua Driver 0.28.2 (release tag
`cua-driver-rs-v0.28.2`, commit `fc188250b4ca8549b8e61f937fdb1fb560770e86`).
The pinned version, the per-target release asset and its sha256 digest live
in `cua-protocol/src/cua_driver_pin.rs`; any download is verified against
that table before it is used. Nolune does not update the driver on its own:
a driver reporting another version is treated as incompatible until Nolune
moves the pin in a release of its own.

Cua Driver is provided under the following license:

```
MIT License

Copyright (c) 2025 Cua AI, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
