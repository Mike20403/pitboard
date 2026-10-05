//! What macOS and Linux do the same way, because both are POSIX: modes, signals, the
//! passwd database, a login shell, and a service manager asked from the command line.

pub(crate) mod fs;
pub(crate) mod proc;
pub(crate) mod service;
pub(crate) mod shell;
pub(crate) mod user;
