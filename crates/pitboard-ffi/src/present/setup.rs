//! How far setting Pitboard up this machine is, and what the window and the menu say in place
//! of a list, from AppModel.swift's footing, AccountsPane.swift and MenuBarContent.swift.

use super::accounts::in_order;
use super::notices::INSTALL_CLAUDE_CODE;
use super::words;
use super::{AccountsShown, Choice, Footing, Seen, SetupStep};
use crate::model::{Intent, Sheet};

/// How far along setting Pitboard up this machine is, worked out across every tool.
pub(crate) fn footing(seen: &Seen) -> Footing {
    // Before the first read there is nothing to go on, and guessing at this point shows
    // somebody a setup step they may have finished years ago.
    let Some(status) = &seen.state.status else {
        return Footing::Ready;
    };
    let accounts = &status.accounts;
    // Said only where nothing else is: a login or an account means a tool is here however it
    // was installed, and a program the app could not find may still be. The core's read does
    // not fail for a missing tool, so this is the one place it shows.
    if accounts.is_empty() && seen.state.installed.as_ref().is_some_and(Vec::is_empty) {
        return Footing::NoClaudeCode;
    }
    if !accounts.iter().any(|account| account.signed_in) {
        // Enrolled accounts with nobody signed in is a machine mid-switch or one whose login
        // was signed out from elsewhere, not a machine that needs setting up.
        return if accounts.is_empty() {
            Footing::NoOneSignedIn
        } else {
            Footing::Ready
        };
    }
    if let Some(login) = seen.unnamed().next() {
        return Footing::Unnamed {
            provider: login.provider.clone(),
            email: login.email.clone(),
        };
    }
    // Not before the preferences are in, which say which tools somebody declined it for: a
    // read that lands first would show the step to them for as long as the file takes.
    if seen.state.reading_preferences() {
        return Footing::Ready;
    }
    // Per tool: an account can only be switched to another account of its own tool.
    let providers: Vec<&str> = accounts.iter().map(|a| a.provider.as_str()).collect();
    let declined = &seen.state.preferences.second_account_declined;
    for provider in in_order(&providers, &seen.tools) {
        if declined.contains(&provider) {
            continue;
        }
        let enrolled: Vec<_> = accounts
            .iter()
            .filter(|account| account.provider == provider && account.label.is_some())
            .collect();
        if let [only] = enrolled[..]
            && only.signed_in
            && let Some(label) = &only.label
        {
            return Footing::OnlyOne {
                provider,
                label: label.clone(),
            };
        }
    }
    Footing::Ready
}

/// The one next thing to do on a machine that is not set up yet. Somebody who opens the app
/// has usually never run a command and may never want to.
pub(crate) fn step(seen: &Seen, footing: &Footing) -> Option<SetupStep> {
    match footing {
        Footing::Unnamed { provider, email } => {
            let to = seen
                .tool_if_shown(provider)
                .map(|tool| format!(" to {tool}"))
                .unwrap_or_default();
            Some(SetupStep {
                title: "Give this account a name".into(),
                detail: format!(
                    "{email} is signed in{to}. Pitboard parks logins under a name you choose, \
                     and can’t park this one until it has one."
                ),
                actions: vec![Choice {
                    title: "Name…".into(),
                    intent: Intent::PresentSheet {
                        sheet: Sheet::Name {
                            provider: provider.clone(),
                            email: email.clone(),
                        },
                    },
                    enabled: true,
                }],
            })
        }
        Footing::OnlyOne { provider, label } => {
            let tool = seen
                .tool_if_shown(provider)
                .map(|tool| format!("{tool} "))
                .unwrap_or_default();
            Some(SetupStep {
                title: format!("Add a second {tool}account"),
                detail: format!(
                    "{label} is the only {tool}account Pitboard knows, so there’s nothing to \
                     switch to. Adding another signs in to it and parks its login beside this \
                     one."
                ),
                actions: vec![
                    Choice {
                        title: "Add Account…".into(),
                        intent: Intent::PresentSheet {
                            sheet: Sheet::Add {
                                provider: Some(provider.clone()),
                            },
                        },
                        enabled: true,
                    },
                    // Somebody may keep one account on purpose and watch its limits, so the
                    // nudge can be declined, for its tool alone.
                    Choice {
                        title: "Not Now".into(),
                        intent: Intent::DeclineSecondAccount {
                            provider: provider.clone(),
                        },
                        enabled: true,
                    },
                ],
            })
        }
        Footing::NoClaudeCode | Footing::NoOneSignedIn | Footing::Ready => None,
    }
}

/// What the accounts pane shows: a list, or why there is none and what to do about it.
///
/// A failed read with nothing to list is the pane's whole content, with why and a way to try
/// again, and not a spinner that never stops or a list with nothing in it, whether nothing is
/// known at all or what is known is empty. A machine without Claude Code says that instead.
pub(crate) fn accounts_shown(seen: &Seen, footing: &Footing) -> AccountsShown {
    let names: Vec<&str> = seen.tools.iter().map(|tool| tool.name.as_str()).collect();
    match footing {
        Footing::NoClaudeCode => AccountsShown::NoTool {
            title: "Claude Code Isn’t Installed".into(),
            detail: format!(
                "Pitboard switches the logins of {}, so there is nothing for it to do until \
                 one of them is installed and signed in once.",
                names.join(" and ")
            ),
            link_title: "How to Install Claude Code".into(),
            link: INSTALL_CLAUDE_CODE.into(),
        },
        _ if seen.problem().is_some() && seen.accounts().is_empty() => AccountsShown::ReadFailed {
            title: "Couldn’t Read Accounts".into(),
            detail: seen.problem().unwrap_or_default().to_owned(),
            // Held back while the accounts are read again, as the toolbar's Refresh is, so
            // pressing it is seen to have done something; AccountsPane.swift held it back.
            retry: Choice {
                title: "Try Again".into(),
                intent: Intent::Refresh { asked: true },
                enabled: seen.state.reads == 0,
            },
        },
        Footing::NoOneSignedIn => AccountsShown::NoAccounts {
            title: "No Accounts".into(),
            detail: "Sign in once here and Pitboard parks that login, so signing in to another \
                     account doesn’t cost you the first."
                .into(),
            add: Choice {
                title: "Add Account…".into(),
                intent: Intent::PresentSheet {
                    sheet: Sheet::Add { provider: None },
                },
                enabled: true,
            },
        },
        _ if seen.state.status.is_none() => AccountsShown::Reading {
            title: "Reading accounts…".into(),
        },
        _ => AccountsShown::List,
    }
}

/// What the menu says where it has no accounts to list.
pub(crate) fn menu_accounts_note(seen: &Seen, footing: &Footing) -> Option<String> {
    match &seen.state.status {
        None if seen.problem().is_none() => Some("Reading accounts…".into()),
        None => Some("No accounts to show".into()),
        Some(status) if status.accounts.is_empty() && *footing != Footing::NoClaudeCode => {
            Some("No accounts yet".into())
        }
        Some(_) => None,
    }
}

/// When the numbers were read, as the menu's Refresh item says under it: as a time rather
/// than an age, since a menu can stay open, and "just now" would still say so ten minutes
/// later.
pub(crate) fn updated_menu(seen: &Seen) -> String {
    if seen.state.reads > 0 {
        return "Reading…".into();
    }
    match seen.state.updated_ms {
        Some(ms) => words::updated(&seen.clock(ms.div_euclid(1000))),
        None if seen.problem().is_none() => "Not read yet".into(),
        None => "Showing the last numbers measured".into(),
    }
}

/// The same, as the window's subtitle says it: nothing before the first read.
pub(crate) fn updated_window(seen: &Seen) -> String {
    if seen.state.reads > 0 {
        return "Reading…".into();
    }
    seen.state
        .updated_ms
        .map(|ms| words::updated(&seen.clock(ms.div_euclid(1000))))
        .unwrap_or_default()
}
