use super::*;
use crate::{
    application::Task,
    remote::native::{project_key, Binding},
    templates::{Target as Owner, Template},
};

#[test]
fn managed_and_forwarded_templates_stay_on_their_vm() {
    let mut app = Workspace::default();
    let (id, r, _) = created(&mut app);
    app.apply(Event::Completed(
        r,
        Ok(Reply::Remote(prepared_vm(machine("vm-a"), "main"))),
    ));
    let managed = Owner::Boat(id.clone());
    let project = project_key(&id);
    let native = Owner::Checkout(project.clone());
    let binding = Binding::new(
        &id,
        &app.remote.saved.checkouts[&id].native_target().unwrap(),
    )
    .unwrap();
    app.show_templates(Some(native.clone()));
    let (r, c) = app.commands().pop().unwrap();
    assert!(
        matches!(binding.outgoing(c).unwrap(), Command::Task(Task::Templates{project}) if project=="/workspace/checkout")
    );
    app.show_templates(Some(managed.clone()));
    let (active, c) = app.commands().pop().unwrap();
    assert!(
        matches!(c,Command::Remote(Operation::Templates(t)) if t.machine.id=="vm-a" && t.root.as_deref()==Some("/workspace/checkout"))
    );
    let prompt = |s: &str| vec![Template::parse("explain".into(), s.into()).unwrap()];
    app.apply(Event::Completed(
        active,
        Ok(Reply::Remote(Outcome::Templates(prompt("VM: $1")))),
    ));
    app.apply(
        binding
            .incoming(Event::Completed(
                r,
                Ok(Reply::Task(crate::application::TaskReply::Templates(
                    prompt("late"),
                ))),
            ))
            .unwrap(),
    );
    assert!(app.templates.items(&native).is_empty());
    assert!(app.send_remote_draft(&id, "/explain 'a b'").unwrap());
    assert!(
        matches!(app.commands().pop().unwrap().1,Command::Remote(Operation::Prompt(t,p)) if t.machine.id=="vm-a" && p.text=="VM: a b")
    );
}
