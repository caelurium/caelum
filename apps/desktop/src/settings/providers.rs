use super::{heading, picker, setting_row, settings_project, settings_provider};
use crate::{
    Workspace,
    live::{live, live_mut},
    select::Select,
    ui::{self, Button},
};
use caelum_core::application::{CodexPermissions, Provider, Scope};
use rune::{El, col, row, text};
use rune_kit::{ButtonVariant, Kit, SelectAppearance};

pub(super) fn providers(s: &Workspace, k: &Kit, layers: &mut Vec<El>) -> El {
    let app = &live(s).app;
    let project = settings_project(s);
    let provider = settings_provider(s);
    let available = app.selectable_providers(project.as_deref());
    let destination = project.as_deref().and_then(|p| app.native_checkout(p));
    let detail = destination.map(|c| {
        format!(
            "Codex defaults for new sessions on {}. Adjust each session in the composer.",
            c.title()
        )
    });
    let mut body = col().w_full().child(heading(
        "Providers",
        detail
            .as_deref()
            .unwrap_or("Defaults for new sessions. Adjust each session in the composer."),
        k,
    ));
    if available.len() > 1 {
        let mut defaults = ui::segmented_track(k);
        for &p in &available {
            let destination = project.clone();
            defaults = defaults.child(
                ui::segment(
                    ui::provider_name(p),
                    app.default_provider_for(destination.as_deref()) == p,
                    k,
                )
                .on_press(move |s: &mut Workspace| {
                    live_mut(s)
                        .app
                        .set_default_provider_for(destination.as_deref(), p)
                })
                .grow(1.0)
                .key(format!("default-provider-{}", p.as_str())),
            );
        }
        body = body
            .child(setting_row(
                "Default provider",
                "Start new sessions with",
                defaults,
                k,
            ))
            .child(ui::divider(k));
    }
    let mut tabs = ui::segmented_track(k);
    for &p in &available {
        tabs = tabs.child(
            ui::segment(ui::provider_name(p), p == provider, k)
                .on_press(move |s: &mut Workspace| {
                    let scope = settings_project(s).map(|project| Scope {
                        provider: p,
                        project,
                    });
                    live_mut(s).panels.settings_provider = p;
                    live_mut(s).panels.settings_menu = None;
                    if let Some(scope) = scope {
                        live_mut(s).app.refresh_choices_for(scope);
                    } else {
                        live_mut(s).app.refresh_choices(p);
                    }
                })
                .key(format!("settings-provider-{}", p.as_str())),
        );
    }
    let Some(project) = &project else {
        return body.child(tabs.py(10.0)).child(ui::caption(
            "Open a project to discover available models and agents.",
            k,
        ));
    };
    let scope = Scope {
        provider,
        project: project.clone(),
    };
    let choices = app.choices.get(&scope);
    let (status_label, status_color) = if app.notices.contains_key(&scope) {
        ("Unavailable", k.c.danger.fg)
    } else if choices.is_some() {
        ("Available", k.c.success.fg)
    } else {
        ("Not connected", k.c.text_muted)
    };
    let status_pill = row()
        .center_y()
        .gap(5.0)
        .px(7.0)
        .py(2.0)
        .radius(5.0)
        .fill(k.alpha(status_color, 0.12))
        .child(
            rune::el()
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(status_color)
                .a11y_hidden(),
        )
        .child(text(status_label).font(k.ty.small).color(status_color));
    let mut panel = ui::card(k).gap(8.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(tabs)
            .child(rune::el().grow(1.0))
            .child(status_pill)
            .child(
                crate::quiet_button(&ui::REFRESH, "Refresh provider status")
                    .on_press(move |s: &mut Workspace| live_mut(s).app.refresh_choices(provider))
                    .build(k)
                    .key("settings-provider-refresh"),
            ),
    );
    if let Some(error) = app.notices.get(&scope) {
        panel = panel.child(ui::notice("Connection unavailable", error, k));
    }
    let options = app.provider_options(&scope).cloned().unwrap_or_default();
    let model = choices.and_then(|c| {
        c.models.iter().find(|m| {
            Some(m.id.as_str()) == options.model.as_deref().or(c.default_model.as_deref())
        })
    });
    let mut model_picker = Select::new::<&str>("settings-model", &[])
        .appearance(SelectAppearance::Field)
        .width(232.0)
        .current(options.model.as_deref())
        .placeholder("Provider default")
        .label("Default model")
        .open(live(s).models.is_open())
        .on_toggle(move |s: &mut Workspace| crate::models::open(s, Some(provider)));
    if let Some(model) = model.filter(|_| options.model.is_some()) {
        model_picker = model_picker.display(&model.name);
    }
    panel = panel.child(setting_row("Model", "", model_picker.trigger(k), k));
    let mut efforts = vec!["Model default".into()];
    if let Some(model) = model {
        efforts.extend(model.efforts.iter().map(|e| e.id.clone()));
    }
    if let Some(saved) = &options.effort
        && !efforts.contains(saved)
    {
        efforts.push(saved.clone());
    }
    let effort_scope = scope.clone();
    panel = panel.child(setting_row(
        "Reasoning effort",
        "",
        picker(
            s,
            k,
            layers,
            "Default effort",
            &efforts,
            options.effort.as_deref().unwrap_or("Model default"),
            move |s, value| {
                live_mut(s).app.edit_provider_options(&effort_scope, |o| {
                    o.effort = (value != "Model default").then(|| value.into())
                });
            },
        ),
        k,
    ));
    if provider == Provider::Codex
        && caelum_core::remote::native::checkout_id(&scope.project).is_some()
    {
        let perm_scope = scope.clone();
        let labels: Vec<String> = CodexPermissions::ALL
            .iter()
            .map(|p| p.label().to_owned())
            .collect();
        let current = options.permissions.unwrap_or_default().label();
        panel = panel.child(setting_row(
            "Permissions",
            "Approval and sandbox policy for Codex in this VM",
            picker(
                s,
                k,
                layers,
                "Default VM permissions",
                &labels,
                current,
                move |s, value| {
                    if let Some(&permissions) =
                        CodexPermissions::ALL.iter().find(|p| p.label() == value)
                    {
                        live_mut(s).app.edit_provider_options(&perm_scope, |o| {
                            o.permissions = Some(permissions);
                        });
                    }
                },
            ),
            k,
        ));
    }
    if provider == Provider::OpenCode {
        let agent_scope = scope.clone();
        let mut agents = vec!["Provider default".into()];
        if let Some(choices) = choices {
            agents.extend(choices.agents.iter().map(|a| a.id.clone()));
        }
        panel = panel.child(setting_row(
            "Agent",
            "",
            picker(
                s,
                k,
                layers,
                "Default agent",
                &agents,
                options.agent.as_deref().unwrap_or("Provider default"),
                move |s, value| {
                    live_mut(s).app.edit_provider_options(&agent_scope, |o| {
                        o.agent = (value != "Provider default").then(|| value.into())
                    });
                },
            ),
            k,
        ));
    }
    let mut actions = row().w_full().gap(6.0).justify_end();
    if let Some(model) = &options.model {
        let id = model.clone();
        let favorite = app
            .favorite_models(&scope)
            .is_some_and(|f| f.contains(model));
        let favorite_scope = scope.clone();
        actions = actions.child(
            Button::new(if favorite {
                "★ Remove favourite"
            } else {
                "☆ Favourite model"
            })
            .variant(ButtonVariant::GhostMuted)
            .on_press(move |s: &mut Workspace| {
                live_mut(s).app.toggle_favorite_model(&favorite_scope, &id);
            })
            .build(k),
        );
    }
    panel = panel.child(ui::divider(k)).child(
        actions.child(
            Button::new("Reset provider defaults")
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Workspace| {
                    live_mut(s).app.reset_provider_options(&scope);
                })
                .build(k),
        ),
    );
    body.child(
        col()
            .w_full()
            .gap(8.0)
            .pt(12.0)
            .child(
                text("Provider defaults")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(panel),
    )
}
