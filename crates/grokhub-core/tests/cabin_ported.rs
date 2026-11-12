//! Copies of egui cabin tests (crates/grokhub-app) that exercise this
//! crate's shared logic through its public API, so the coverage stays
//! when the cabin is retired. One module per cabin source file; each test
//! keeps the cabin test's name, and the cabin originals are unchanged.

/// From `crates/grokhub-app/src/app/chat_threads_tests.rs`.
mod app_chat_threads_tests {

    #[test]
    fn selecting_plan_does_not_change_the_thread_title() {
        assert_eq!(
            grokhub_core::cli_history::title_after_selecting_plan("Night watch"),
            "Night watch"
        );
        assert_eq!(
            grokhub_core::cli_history::preferred_history_title(
                "Night watch",
                false,
                Some("Plan"),
                Some("abc")
            ),
            "Night watch"
        );
        assert_eq!(
            grokhub_core::cli_history::history_label_after_plan("Night watch", "fix the dock"),
            "Night watch"
        );
    }
}

/// From `crates/grokhub-app/src/app/mission_page/goals.rs`.
mod app_mission_page_goals {
    use grokhub_core::objectives::goal_detail;
    use grokhub_core::objectives::goal_title;
    use grokhub_core::objectives::steps_by_one;
    use grokhub_core::objectives::GameRef;
    use grokhub_core::objectives::Goal;
    use grokhub_core::objectives::GoalKind;
    use grokhub_core::objectives::NewGoal;
    use grokhub_core::objectives::Objectives;
    use grokhub_core::objectives::Origin;

    fn goal(kind: GoalKind, game: &str, current: u32, target: u32) -> Goal {
        let mut o = Objectives::default();
        o.add(
            NewGoal {
                game: GameRef {
                    name: game.into(),
                    ..GameRef::default()
                },
                text: "99 Fishing".into(),
                kind,
                current,
                target,
                origin: Origin::Typed,
                chat: None,
            },
            1,
        )
        .unwrap();
        o.goals.remove(0)
    }

    #[test]
    fn rows_name_the_game_and_say_what_is_left() {
        let mut g = goal(GoalKind::Skill, "Old School RuneScape", 93, 99);
        assert_eq!(goal_title(&g), "Old School RuneScape · 99 Fishing");
        assert_eq!(goal_detail(&g), "93 / 99 · 6 levels to 99");
        assert!(steps_by_one(&g));
        g.checked = true;
        assert_eq!(goal_detail(&g), "93 / 99 · 6 levels to 99 · Checked");
        let g = goal(GoalKind::Finish, "", 0, 1);
        assert_eq!(goal_title(&g), "99 Fishing");
        assert_eq!(goal_detail(&g), "Not finished yet");
        assert!(!steps_by_one(&g), "a finish goal has no +1");
    }
}

/// From `crates/grokhub-app/src/app/native_signin_tests.rs`.
mod app_native_signin_tests {

    #[test]
    fn imagine_need_signin_message_points_at_account_settings() {
        assert_eq!(
            grokhub_core::IMAGINE_NEED_SIGNIN,
            "Sign in with Grok in Settings → AI models, or add a console API key."
        );
        assert!(
            !grokhub_core::IMAGINE_NEED_SIGNIN.contains("for Imagine"),
            "must not push a second Imagine-only sign-in"
        );
    }
}

/// From `crates/grokhub-app/src/app/nav_palette_settings_tests.rs`.
mod app_nav_palette_settings_tests {

    #[test]
    fn palette_query_revert_forgets_empty_saved_hits() {
        let mut files = vec!["nested/deep/buried.txt".to_string()];
        let mut files_q = "buried".to_string();
        let mut files_root = "/place".to_string();
        grokhub_core::palette_forget_stale_walk(
            &mut files,
            &mut files_q,
            &mut files_root,
            "buriexx",
            "/place",
        );
        assert!(files.is_empty(), "a query change clears the last hits");
        assert!(
            files_q.is_empty() && files_root.is_empty(),
            "the finished key must be forgotten or a revert matches the empty list"
        );
        assert!(
            !grokhub_core::palette_search_is_saved(&files_q, &files_root, "buried", "/place"),
            "reverting to the earlier query must walk again so the file hits come back"
        );
        grokhub_core::palette_forget_stale_walk(
            &mut files,
            &mut files_q,
            &mut files_root,
            "buried",
            "/place",
        );
        assert!(
            files_q.is_empty() && files.is_empty(),
            "revert still has no finished key, so tick kicks a real walk: {files_q:?} {files:?}"
        );
    }

    #[test]
    fn overlay_update_skips_chat() {
        let v = grokhub_core::overlay_update_begin(2);
        assert!(v.stay_on_update);
        assert!(!v.posts_chat);
        let done = grokhub_core::overlay_update_finish(true, 50);
        assert!(!done.posts_chat);
        assert!(done.stay_on_update);
        assert!(done.can_restart);
        assert!(grokhub_core::overlay_update_can_restart(true, false));
        assert!(!grokhub_core::overlay_update_can_restart(true, true));
    }
}

/// From `crates/grokhub-app/src/app/tests.rs`.
mod app_tests {

    #[test]
    fn appearance_tab_offers_loadout_and_classic() {
        let ids: Vec<&str> = grokhub_core::appearance_choices()
            .iter()
            .copied()
            .map(grokhub_core::theme_id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "ops-dark",
                "oled-black",
                "soft-charcoal",
                "dark",
                "light",
                "system"
            ]
        );
        assert_eq!(
            grokhub_core::parse_theme("light"),
            grokhub_core::ThemeChoice::Light
        );
        assert_eq!(
            grokhub_core::parse_theme("ops-dark"),
            grokhub_core::ThemeChoice::OpsDark
        );
        assert!(!grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::Light,
            true
        ));
        assert!(grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::Dark,
            false
        ));
        assert!(grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::OpsDark,
            false
        ));
        assert!(!grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::System,
            false
        ));
    }
}

/// From `crates/grokhub-app/src/update.rs`.
mod update {
    use grokhub_core::auto_off_step;
    use grokhub_core::AutoOffStep;
    use grokhub_core::Channel;

    #[test]
    fn auto_off_step_from_core_drives_receipt_policy() {
        let tips = |b: &str, m: &str, bt: &str, mt: &str| grokhub_core::ChannelTips {
            beta_sha: b.into(),
            main_sha: m.into(),
            beta_tree: bt.into(),
            main_tree: mt.into(),
        };
        let tree = "47fd2b27aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let older = "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            auto_off_step(
                Channel::Beta,
                &tips("e79aa50f", "7266ae6a", tree, tree),
                Some(older)
            ),
            AutoOffStep::SwitchToStable
        );
        assert_eq!(
            auto_off_step(
                Channel::Beta,
                &tips("e79aa50f", "7266ae6a", tree, tree),
                Some(tree)
            ),
            AutoOffStep::Stay
        );
        assert_eq!(
            auto_off_step(
                Channel::Beta,
                &tips("e79aa50f", "7266ae6a", tree, tree),
                None
            ),
            AutoOffStep::RecordBaseline(tree.into())
        );
        assert_eq!(
            auto_off_step(
                Channel::Beta,
                &tips("aaaaaaa", "bbbbbbb", older, tree),
                Some(older)
            ),
            AutoOffStep::Stay
        );
    }
}
