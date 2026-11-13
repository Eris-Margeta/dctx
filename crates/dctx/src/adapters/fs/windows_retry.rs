// SPDX-License-Identifier: Apache-2.0
//! Windows retry policy with injectable operations and backoff for portable tests.
use std::io;
use std::time::Duration;

pub(super) fn retry<T>(
    mut open: impl FnMut() -> io::Result<T>,
    mut backoff: impl FnMut(Duration),
) -> io::Result<T> {
    let original = match open() {
        Ok(value) => return Ok(value),
        Err(error) if transient(&error) => error,
        Err(error) => return Err(error),
    };
    // Only Windows sharing/lock violations are transient. Preserve a later ACL
    // denial immediately; it must not be hidden by an earlier sharing conflict.
    for millis in [10, 20, 40, 80, 160] {
        backoff(Duration::from_millis(millis));
        match open() {
            Ok(value) => return Ok(value),
            Err(error) if transient(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Err(original)
}

fn transient(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(32 | 33))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_and_permanent_errors_do_not_back_off() {
        assert_eq!(retry(|| Ok(7), |_| panic!("unexpected retry")).unwrap(), 7);
        let error = retry::<()>(
            || Err(io::Error::from_raw_os_error(5)),
            |_| panic!("permission denial is permanent"),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
    }

    #[test]
    fn sharing_and_lock_conflicts_can_recover() {
        let mut outcomes = [Err(32), Err(33), Ok(7)].into_iter();
        let mut delays = Vec::new();
        let result = retry(
            || {
                outcomes
                    .next()
                    .unwrap()
                    .map_err(io::Error::from_raw_os_error)
            },
            |delay| delays.push(delay.as_millis()),
        );
        assert_eq!(result.unwrap(), 7);
        assert_eq!(delays, [10, 20]);
        assert!(outcomes.next().is_none());
    }

    #[test]
    fn exhausted_conflicts_return_the_first_error_after_five_retries() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let error = retry::<()>(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(if attempts == 1 {
                    32
                } else {
                    33
                }))
            },
            |delay| delays.push(delay.as_millis()),
        )
        .unwrap_err();
        assert_eq!(attempts, 6);
        assert_eq!(delays, [10, 20, 40, 80, 160]);
        assert_eq!(error.raw_os_error(), Some(32));
    }

    #[test]
    fn permission_denial_after_sharing_conflict_is_preserved() {
        let mut outcomes = [32, 5].into_iter();
        let mut delays = Vec::new();
        let error = retry::<()>(
            || Err(io::Error::from_raw_os_error(outcomes.next().unwrap())),
            |delay| delays.push(delay.as_millis()),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        assert_eq!(delays, [10]);
        assert!(outcomes.next().is_none());
    }
}
