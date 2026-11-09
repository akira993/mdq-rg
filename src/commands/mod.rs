pub mod get;
pub mod list;
pub mod search;

fn output_error(error: std::io::Error) -> i32 {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        0
    } else {
        crate::search::output::report_stderr(format_args!("{error}"));
        1
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn output_errors_only_treat_broken_pipe_as_success() {
        use std::io::{Error, ErrorKind};
        assert_eq!(super::output_error(Error::from(ErrorKind::BrokenPipe)), 0);
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::WriteZero,
            ErrorKind::Other,
        ] {
            assert_eq!(super::output_error(Error::from(kind)), 1);
        }
    }
}
