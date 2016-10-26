use std::fs::File;
use std::io;
use std::process::Stdio;

pub struct Pair {
    pub read: File,
    pub write: File,
}

pub fn pipe() -> io::Result<Pair> {
    sys::pipe()
}

pub fn parent_stdin() -> io::Result<Stdio> {
    sys::parent_stdin()
}

pub fn parent_stdout() -> io::Result<Stdio> {
    sys::parent_stdout()
}

pub fn parent_stderr() -> io::Result<Stdio> {
    sys::parent_stderr()
}

pub fn stdio_from_file(file: File) -> Stdio {
    sys::stdio_from_file(file)
}

#[cfg(not(windows))]
#[path = "unix.rs"]
mod sys;
#[cfg(windows)]
#[path = "windows.rs"]
mod sys;

#[cfg(test)]
mod tests {
    use std::io::prelude::*;
    use std::env::consts::EXE_EXTENSION;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Once, ONCE_INIT};
    use std::thread;
    use ::Pair;

    fn path_to_exe(name: &str) -> PathBuf {
        // This project defines some associated binaries for testing, and we shell out to them in
        // these tests. `cargo test` doesn't automatically build associated binaries, so this
        // function takes care of building them explicitly.
        static CARGO_BUILD_ONCE: Once = ONCE_INIT;
        CARGO_BUILD_ONCE.call_once(|| {
            let build_status = Command::new("cargo")
                .arg("build")
                .arg("--quiet")
                .status()
                .unwrap();
            assert!(build_status.success(),
                    "Cargo failed to build associated binaries.");
        });

        Path::new("target").join("debug").join(name).with_extension(EXE_EXTENSION)
    }

    #[test]
    fn test_pipe_some_data() {
        let mut pair = ::pipe().unwrap();
        // A small write won't fill the pipe buffer, so it won't block this thread.
        pair.write.write_all(b"some stuff").unwrap();
        drop(pair.write);
        let mut out = String::new();
        pair.read.read_to_string(&mut out).unwrap();
        assert_eq!(out, "some stuff");
    }

    #[test]
    fn test_pipe_no_data() {
        let mut pair = ::pipe().unwrap();
        drop(pair.write);
        let mut out = String::new();
        pair.read.read_to_string(&mut out).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn test_pipe_a_megabyte_of_data_from_another_thread() {
        let data = vec![0xff; 1_000_000];
        let data_copy = data.clone();
        let Pair { mut read, mut write } = ::pipe().unwrap();
        let joiner = thread::spawn(move || {
            write.write_all(&data_copy).unwrap();
        });
        let mut out = Vec::new();
        read.read_to_end(&mut out).unwrap();
        joiner.join().unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn test_pipes_are_not_inheritable() {
        // Create pipes for a child process.
        let mut input_pipe = ::pipe().unwrap();
        let mut output_pipe = ::pipe().unwrap();
        let child_stdin = ::stdio_from_file(input_pipe.read);
        let child_stdout = ::stdio_from_file(output_pipe.write);

        // Spawn the child. Note that this temporary Command object takes ownership of our copies
        // of the child's stdin and stdout, and then closes them immediately when it drops. That
        // stops us from blocking our own read below. We use our own simple implementation of cat
        // for compatibility with Windows.
        let mut child = Command::new(path_to_exe("cat"))
            .stdin(child_stdin)
            .stdout(child_stdout)
            .spawn()
            .unwrap();

        // Write to the child's stdin. This is a small write, so it shouldn't block.
        input_pipe.write.write_all(b"hello").unwrap();
        drop(input_pipe.write);

        // Read from the child's stdout. If this child has accidentally inherited the write end of
        // its own stdin, then it will never exit, and this read will block forever. That's the
        // what this test is all about.
        let mut output = Vec::new();
        output_pipe.read.read_to_end(&mut output).unwrap();
        child.wait().unwrap();

        // Confirm that we got the right bytes.
        assert_eq!(b"hello", &*output);
    }

    #[test]
    fn test_parent_handles() {
        // This test invokes the `swap` test program, which uses parent_stdout() and
        // parent_stderr() to swap the outputs for another child that it spawns.

        // Create pipes for a child process.
        let mut input_pipe = ::pipe().unwrap();
        let child_stdin = ::stdio_from_file(input_pipe.read);

        // Write input. This shouldn't block because it's small. Then close the write end, or else
        // the child will hang.
        input_pipe.write.write_all(b"quack").unwrap();
        drop(input_pipe.write);

        // Use `swap` to run `cat`. `cat will read "quack" from stdin and write it to stdout. But
        // because we run it inside `swap`, that write should end up on stderr.
        let output = Command::new(path_to_exe("swap"))
            .arg(path_to_exe("cat"))
            .stdin(child_stdin)
            .output()
            .unwrap();

        // Check for a clean exit.
        assert!(output.status.success(),
                "child process returned {:#?}",
                output);

        // Confirm that we got the right bytes.
        assert_eq!(b"", &*output.stdout);
        assert_eq!(b"quack", &*output.stderr);
    }
}
