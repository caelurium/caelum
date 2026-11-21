use super::*;
use crate::application::TaskReply;

#[test]
fn superseded_request_cannot_fail_or_unlock_the_newer_request() {
    let mut services = Services::default();
    let old = Task::Completions {
        project: "/checkout".into(),
        query: "old".into(),
    };
    let new = Task::Completions {
        project: "/checkout".into(),
        query: "new".into(),
    };
    services.start(&old, 1);
    services.start(&new, 2);
    services.complete(1, &old, Err("late failure".into()));
    assert!(services.busy(&new));
    assert!(services.error(&new).is_none());

    services.complete(2, &new, Err("current failure".into()));
    services.complete(2, &new, Ok(TaskReply::Done));
    assert!(!services.busy(&new));
    assert_eq!(services.error(&new), Some("current failure"));
}
