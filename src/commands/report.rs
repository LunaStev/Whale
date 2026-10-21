//! Fallible stdout boundary for CLI status, help and debug reporting.
use std::{
    fmt,
    io::{self, Write},
};

pub(crate) fn write(arguments: fmt::Arguments<'_>) {
    let mut stdout = io::stdout().lock();
    if let Err(error) = stdout.write_fmt(arguments).and_then(|_| stdout.flush()) {
        // Reporting is optional when the reader closes its pipe. Continue the
        // command so artifacts and genuine command failures keep their policy.
        if error.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("Error: failed to write stdout: {error}");
            std::process::exit(1);
        }
    }
}

macro_rules! print {
    ($($argument:tt)*) => { $crate::commands::report::write(format_args!($($argument)*)) };
}

macro_rules! println {
    () => { $crate::commands::report::write(format_args!("\n")) };
    ($($argument:tt)*) => { $crate::commands::report::write(format_args!("{}\n", format_args!($($argument)*))) };
}

pub(crate) use print;

pub(crate) use println;
