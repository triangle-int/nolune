//go:build darwin || linux

package main

import (
	"syscall"
	"unsafe"
)

// isTerminal reports whether fd is a terminal, the way golang.org/x/term does: asking for its
// terminal settings only works on one. /dev/null, where the agent's commands read stdin from,
// is a character device but not a terminal.
func isTerminal(fd uintptr) bool {
	var settings syscall.Termios
	_, _, errno := syscall.Syscall(syscall.SYS_IOCTL, fd, ioctlGetTermios, uintptr(unsafe.Pointer(&settings)))
	return errno == 0
}
