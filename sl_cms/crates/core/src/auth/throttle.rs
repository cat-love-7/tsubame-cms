//! Login throttling.
//!
//! A password can be guessed one attempt at a time, so the CMS counts the failures per
//! account and refuses further attempts for a while once they pile up. Two properties are
//! deliberate:
//!
//! * **Every identifier is counted, known or not.** Throttling only real accounts would turn
//!   the limiter itself into the account-enumeration oracle that the identical
//!   `invalid username or password` answer exists to prevent.
//! * **The lock has a fixed end.** Further attempts while locked do not push it out, so
//!   someone cannot keep an account locked by hammering it.
//!
//! **This is the on-premises deployment's policy.** Where sign-in is Cognito's (the AWS
//! backend), the CMS never sees an attempt — the browser signs in against Cognito and the CMS
//! only verifies the token it gets back — so there is nothing here to count and nothing that
//! could count it: Cognito does not expose a failure count, only its own lockout state, and
//! its documented answer to volume is AWS WAF rather than a per-account limiter. The local
//! password endpoints answer 501 on AWS for that reason, which is what keeps this module
//! unreachable there rather than merely unused.
//!
//! The counters live in memory: a restart forgets them, and two processes would not share
//! them. That is enough for the single-process deployment this runs as; a
//! multi-instance deployment (or Lambda, where instances come and go) would need the
//! counters in shared storage.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failures allowed before the account is locked out.
pub const MAX_FAILURES: u32 = 5;
/// How long a failure is remembered: failures further apart than this do not add up.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// How long an account stays locked, counted from the failure that tripped the limit.
pub const LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Accounts tracked at once. Reached only by someone cycling through made-up names, which is
/// what the pruning below is for.
const MAX_TRACKED_ACCOUNTS: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct Attempts {
    failures: u32,
    last_failure: Instant,
}

impl Attempts {
    /// When the lock ends, once the limit has been reached.
    fn lock_ends(&self) -> Option<Instant> {
        (self.failures >= MAX_FAILURES).then(|| self.last_failure + LOCKOUT)
    }
}

#[derive(Debug, Default)]
pub struct LoginThrottle {
    attempts: Mutex<HashMap<String, Attempts>>,
}

impl LoginThrottle {
    pub fn new() -> Self {
        LoginThrottle::default()
    }

    /// How long this account has to wait, or `None` when it may try now.
    ///
    /// `now` is passed in rather than read here so the tests can move time without sleeping.
    pub fn retry_after(&self, email: &str, now: Instant) -> Option<Duration> {
        let attempts = self.attempts.lock().ok()?;
        let ends = attempts.get(email)?.lock_ends()?;
        (ends > now).then(|| ends - now)
    }

    /// Count one failed attempt for this account.
    pub fn record_failure(&self, email: &str, now: Instant) {
        let Ok(mut attempts) = self.attempts.lock() else {
            // A poisoned lock only means some other thread panicked; failing open keeps
            // sign-in working, which matters more than the counter.
            return;
        };

        if attempts.len() >= MAX_TRACKED_ACCOUNTS && !attempts.contains_key(email) {
            // Someone is cycling through names; drop what has expired before growing.
            attempts.retain(|_, record| now.duration_since(record.last_failure) < WINDOW);
        }

        let record = attempts.entry(email.to_string()).or_insert(Attempts {
            failures: 0,
            last_failure: now,
        });
        // Attempts made while the account is locked do not push the end of the lock out,
        // so hammering an account cannot keep its owner out for longer than `LOCKOUT`.
        if record.lock_ends().is_some_and(|ends| ends > now) {
            return;
        }
        // Failures further apart than the window do not add up: a person mistyping their
        // password once a day is not an attack.
        if now.duration_since(record.last_failure) >= WINDOW {
            record.failures = 0;
        }
        record.failures = record.failures.saturating_add(1);
        record.last_failure = now;
    }

    /// Forget the failures for this account, which is what a successful sign-in means.
    pub fn record_success(&self, email: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(email);
        }
    }

    /// How many accounts are being tracked; the pruning above is what bounds it.
    #[cfg(test)]
    fn tracked_accounts(&self) -> usize {
        self.attempts.lock().map(|a| a.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> Instant {
        // A fixed origin: only the differences between these instants matter.
        Instant::now() + Duration::from_secs(seconds)
    }

    #[test]
    fn allows_attempts_until_the_limit_then_locks() {
        let throttle = LoginThrottle::new();
        for attempt in 0..MAX_FAILURES {
            assert_eq!(throttle.retry_after("a@example.com", at(attempt as u64)), None);
            throttle.record_failure("a@example.com", at(attempt as u64));
        }

        // The limit is now reached: further attempts have to wait.
        let wait = throttle.retry_after("a@example.com", at(10)).expect("locked");
        assert!(wait <= LOCKOUT);

        // One account being locked says nothing about another.
        assert_eq!(throttle.retry_after("b@example.com", at(10)), None);
    }

    #[test]
    fn the_lock_ends_and_is_not_extended_by_more_attempts() {
        let throttle = LoginThrottle::new();
        for attempt in 0..MAX_FAILURES {
            throttle.record_failure("a@example.com", at(attempt as u64));
        }

        // The lock ends when it was always going to, even though an attempt was made
        // while it was in force.
        let ends = (MAX_FAILURES - 1) as u64 + LOCKOUT.as_secs();
        assert!(throttle.retry_after("a@example.com", at(60)).is_some());
        throttle.record_failure("a@example.com", at(60));
        assert!(throttle.retry_after("a@example.com", at(ends - 1)).is_some());
        assert_eq!(throttle.retry_after("a@example.com", at(ends + 1)), None);
    }

    #[test]
    fn a_successful_sign_in_forgets_the_failures() {
        let throttle = LoginThrottle::new();
        for attempt in 0..MAX_FAILURES - 1 {
            throttle.record_failure("a@example.com", at(attempt as u64));
        }
        throttle.record_success("a@example.com");

        // The next failure starts counting from scratch, so the limit is not reached.
        throttle.record_failure("a@example.com", at(10));
        assert_eq!(throttle.retry_after("a@example.com", at(10)), None);
        assert_eq!(throttle.tracked_accounts(), 1);
    }

    #[test]
    fn failures_further_apart_than_the_window_do_not_add_up() {
        let throttle = LoginThrottle::new();
        let window = WINDOW.as_secs();
        for attempt in 0..MAX_FAILURES {
            // One failure per window: never two inside the same one.
            throttle.record_failure("a@example.com", at(u64::from(attempt) * window));
        }
        assert_eq!(throttle.retry_after("a@example.com", at(MAX_FAILURES as u64 * window)), None);
    }

    /// Made-up names must not grow the map without bound.
    #[test]
    fn tracking_many_accounts_is_bounded() {
        let throttle = LoginThrottle::new();
        for index in 0..MAX_TRACKED_ACCOUNTS + 100 {
            throttle.record_failure(&format!("nobody-{index}"), at(index as u64));
        }
        assert!(
            throttle.tracked_accounts() <= MAX_TRACKED_ACCOUNTS,
            "{} 件を追跡している",
            throttle.tracked_accounts()
        );
    }
}
