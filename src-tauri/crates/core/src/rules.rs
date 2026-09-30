//! Window-based auto profile switching: the `rules.json` schema, the matching,
//! and the debounced decision engine.
//!
//! Everything here except the types is pure. A rule is matched against a
//! [`Snapshot`] — a list of windows as plain data — and the desktop that
//! produces one lives in [`crate::winwatch`]. That is the same split as
//! `monitors::enumerate` vs `monitors::resolve`, and for the same reason: the
//! states worth testing (a game behind a browser, a title that changes while
//! the process stays, a rule list that reorders) are exactly the ones a
//! developer's own desktop does not have on demand.
//!
//! The decision is window-centric: a rule matches when **one window**
//! satisfies its whole condition. "Process is `cs2.exe` and the title contains
//! Counter-Strike" means both of those about the same window, not one window
//! with the process and another with the title — the second reading matches
//! things nobody meant, and it is not expressible here.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The contents of `rules.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRules {
    /// Whether the engine arbitrates at all. Off by default, so an app with no
    /// rules file never switches a profile on its own.
    #[serde(default)]
    pub enabled: bool,
    /// The profile to switch to when no rule matches.
    ///
    /// Required in spirit once rules exist, but optional in the file: a set of
    /// rules with no fallback only ever *enters* profiles, and the engine says
    /// nothing rather than inventing one.
    #[serde(default)]
    pub fallback_profile: Option<String>,
    /// Evaluated in order; the first match wins.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// One entry of `rules.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Free-form label for the editor and for the switch log line.
    #[serde(default)]
    pub name: String,
    /// Which windows are candidates.
    #[serde(default)]
    pub scope: Scope,
    /// How the conditions below combine.
    #[serde(default)]
    pub combine: Combine,
    /// The conditions, each tested against the same candidate window.
    #[serde(default)]
    pub when: Vec<Condition>,
    /// The profile id to activate. An id, never a display name.
    #[serde(default)]
    pub profile: String,
}

/// Which windows a rule looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// Any visible top-level window that is not a tool window. This is the
    /// "exists" reading: a game behind a browser still matches, which is what
    /// makes a game profile survive an alt-tab.
    #[default]
    AnyWindow,
    /// Only the focused window.
    Foreground,
}

/// How a rule's conditions combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Combine {
    #[default]
    All,
    Any,
}

/// What a condition looks at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Part {
    /// The window's owning process, by executable file name (`cs2.exe`).
    #[default]
    ProcessName,
    /// The window's title.
    Title,
    /// The Win32 window class name (`Chrome_WidgetWin_1`).
    ClassName,
}

/// How a condition's value is compared against the part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchMode {
    /// Whole-field equality, case-insensitive.
    #[default]
    Exact,
    /// Substring, case-insensitive.
    Contains,
    /// Regular expression, as written: `(?i)` is how to make one
    /// case-insensitive. An invalid pattern invalidates its rule rather than
    /// the file.
    Regex,
}

/// One condition of a rule.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// No `serde(default)`: an unknown or missing part is rejected rather than
    /// guessed at, the way the pipe rejects an unknown message kind.
    pub part: Part,
    #[serde(default)]
    pub matcher: MatchMode,
    /// An empty value invalidates the rule; a half-typed condition must not
    /// match everything.
    #[serde(default)]
    pub value: String,
}

/// One candidate window, as plain data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowInfo {
    /// The owning process's executable file name, e.g. `cs2.exe`.
    pub process: String,
    pub title: String,
    pub class_name: String,
}

/// The windows a decision is made against.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Visible top-level windows that are not tool windows.
    pub windows: Vec<WindowInfo>,
    /// The focused window, or `None` when the desktop has none.
    pub foreground: Option<WindowInfo>,
}

/// A rule with its regexes compiled and its needles lowercased.
#[derive(Clone, Debug)]
pub struct CompiledRule {
    pub name: String,
    pub profile: String,
    pub scope: Scope,
    pub combine: Combine,
    checks: Vec<Check>,
    /// Why this rule can never match. The rule is kept so the editor can show
    /// it, and [`decide`] skips it.
    pub error: Option<String>,
}

/// Rules ready to be evaluated.
#[derive(Clone, Debug, Default)]
pub struct CompiledRules {
    pub enabled: bool,
    pub fallback: Option<String>,
    pub rules: Vec<CompiledRule>,
}

#[derive(Clone, Debug)]
enum Check {
    Exact { part: Part, needle: String },
    Contains { part: Part, needle: String },
    Regex { part: Part, regex: Regex },
}

/// What the rules say should be active right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub profile: String,
    /// The matching rule's position in the list, or `None` for the fallback.
    pub rule_index: Option<usize>,
}

/// Compile a rules file into something evaluable.
///
/// A rule that cannot mean anything — no profile, no condition, an empty
/// value, an invalid regex — is kept with its [`CompiledRule::error`] filled
/// in, so one bad row disables itself and not the file.
pub fn compile(rules: &AutoRules) -> CompiledRules {
    CompiledRules {
        enabled: rules.enabled,
        fallback: rules.fallback_profile.clone(),
        rules: rules.rules.iter().map(compile_rule).collect(),
    }
}

fn compile_rule(rule: &Rule) -> CompiledRule {
    let mut error = None;
    if rule.profile.trim().is_empty() {
        error = Some("no profile to switch to".to_string());
    } else if rule.when.is_empty() {
        error = Some("the rule has no conditions".to_string());
    }
    let mut checks = Vec::new();
    for (index, condition) in rule.when.iter().enumerate() {
        if condition.value.trim().is_empty() {
            error.get_or_insert(format!("condition {} has an empty value", index + 1));
            continue;
        }
        let part = condition.part;
        match condition.matcher {
            MatchMode::Exact => checks.push(Check::Exact {
                part,
                needle: condition.value.to_lowercase(),
            }),
            MatchMode::Contains => checks.push(Check::Contains {
                part,
                needle: condition.value.to_lowercase(),
            }),
            MatchMode::Regex => match Regex::new(&condition.value) {
                Ok(regex) => checks.push(Check::Regex { part, regex }),
                Err(regex_error) => {
                    error.get_or_insert(format!(
                        "condition {} is not a valid regular expression: {regex_error}",
                        index + 1
                    ));
                }
            },
        }
    }
    CompiledRule {
        name: rule.name.clone(),
        profile: rule.profile.clone(),
        scope: rule.scope,
        combine: rule.combine,
        checks,
        error,
    }
}

/// The profile the rules point at right now, or `None` when the engine should
/// stay out of the way.
///
/// Two of those cases are deliberate: rules that are switched off, and an
/// enabled set where nothing can match (no rules at all, or only broken ones).
/// A half-typed rule must not silently mass-apply the fallback, so "enabled
/// but nothing valid" is inert rather than "always fall back".
pub fn decide(rules: &CompiledRules, snapshot: &Snapshot) -> Option<Decision> {
    if !rules.enabled {
        return None;
    }
    if !rules.rules.iter().any(|rule| rule.error.is_none()) {
        return None;
    }
    for (index, rule) in rules.rules.iter().enumerate() {
        if rule.error.is_some() {
            continue;
        }
        if rule.matches(snapshot) {
            return Some(Decision {
                profile: rule.profile.clone(),
                rule_index: Some(index),
            });
        }
    }
    rules.fallback.as_ref().map(|profile| Decision {
        profile: profile.clone(),
        rule_index: None,
    })
}

impl CompiledRule {
    fn matches(&self, snapshot: &Snapshot) -> bool {
        let candidates: &[WindowInfo] = match self.scope {
            Scope::AnyWindow => &snapshot.windows,
            Scope::Foreground => match &snapshot.foreground {
                Some(window) => std::slice::from_ref(window),
                None => &[],
            },
        };
        candidates.iter().any(|window| self.matches_window(window))
    }

    /// The same-window half of the model: every condition is asked about the
    /// window handed in, and `combine` decides how their answers combine.
    fn matches_window(&self, window: &WindowInfo) -> bool {
        let mut checks = self.checks.iter();
        match self.combine {
            Combine::All => checks.all(|check| check.matches(window)),
            Combine::Any => checks.any(|check| check.matches(window)),
        }
    }
}

impl Check {
    fn part(&self) -> Part {
        match self {
            Check::Exact { part, .. }
            | Check::Contains { part, .. }
            | Check::Regex { part, .. } => *part,
        }
    }

    fn matches(&self, window: &WindowInfo) -> bool {
        let field = match self.part() {
            Part::ProcessName => &window.process,
            Part::Title => &window.title,
            Part::ClassName => &window.class_name,
        };
        match self {
            Check::Exact { needle, .. } => &field.to_lowercase() == needle,
            Check::Contains { needle, .. } => field.to_lowercase().contains(needle.as_str()),
            Check::Regex { regex, .. } => regex.is_match(field),
        }
    }
}

/// Consecutive evaluations a decision must hold before it is applied.
///
/// Two ticks. Profile switching is human-scale, and the transitions that
/// would otherwise be visible as flapping are a launcher handing over to the
/// game it launched, and a title that changes while the user alt-tabs
/// mid-load. One tick filters neither; two costs one extra second and filters
/// both.
pub const DEBOUNCE_TICKS: u32 = 2;

/// What one evaluation should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Nothing: no decision, or one that is settled and already applied.
    Idle,
    /// Apply this profile. The caller confirms with [`Engine::mark_applied`]
    /// only if it actually reached the renderer, so a failed push is retried.
    Apply { profile: String },
}

/// The debounced decision state machine.
///
/// Deliberately holds no clock: the caller decides how often to evaluate, and
/// "two consecutive ticks" is then a statement about the sequence of
/// decisions rather than about wall time. That is what makes a whole flap
/// history testable in microseconds.
#[derive(Clone, Debug, Default)]
pub struct Engine {
    /// What the renderer is believed to have, as far as automation goes.
    applied: Option<String>,
    /// The decision currently being held, and for how many ticks.
    candidate: Option<String>,
    held: u32,
}

impl Engine {
    /// One evaluation. Returns what should be applied, if anything.
    pub fn step(&mut self, decision: Option<&str>) -> Tick {
        if decision == self.candidate.as_deref() {
            self.held += 1;
        } else {
            self.candidate = decision.map(str::to_string);
            self.held = 1;
        }
        match &self.candidate {
            Some(profile)
                if self.held >= DEBOUNCE_TICKS && self.applied.as_deref() != Some(profile) =>
            {
                Tick::Apply {
                    profile: profile.clone(),
                }
            }
            _ => Tick::Idle,
        }
    }

    /// Record that a profile reached the renderer. Only call this on success:
    /// a failed push that was recorded as applied would never be retried.
    pub fn mark_applied(&mut self, profile: &str) {
        self.applied = Some(profile.to_string());
    }

    pub fn applied(&self) -> Option<&str> {
        self.applied.as_deref()
    }

    /// Forget what is applied, so the next settled decision goes out again.
    ///
    /// Called when the machine has been running without this engine as the
    /// authority — most importantly when the Config window closes, because it
    /// can switch profiles itself and this engine has no way to have seen it.
    pub fn resume(&mut self) {
        self.applied = None;
        self.candidate = None;
        self.held = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(process: &str, title: &str, class_name: &str) -> WindowInfo {
        WindowInfo {
            process: process.to_string(),
            title: title.to_string(),
            class_name: class_name.to_string(),
        }
    }

    fn snapshot(windows: Vec<WindowInfo>, foreground: Option<WindowInfo>) -> Snapshot {
        Snapshot {
            windows,
            foreground,
        }
    }

    fn condition(part: Part, matcher: MatchMode, value: &str) -> Condition {
        Condition {
            part,
            matcher,
            value: value.to_string(),
        }
    }

    fn rule(scope: Scope, combine: Combine, when: Vec<Condition>, profile: &str) -> Rule {
        Rule {
            name: String::new(),
            scope,
            combine,
            when,
            profile: profile.to_string(),
        }
    }

    fn rules(rules: Vec<Rule>, fallback: Option<&str>) -> AutoRules {
        AutoRules {
            enabled: true,
            fallback_profile: fallback.map(str::to_string),
            rules,
        }
    }

    fn decide_from(rules: &AutoRules, snapshot: &Snapshot) -> Option<Decision> {
        decide(&compile(rules), snapshot)
    }

    fn explorer() -> WindowInfo {
        window("explorer.exe", "File Explorer", "CabinetWClass")
    }

    #[test]
    fn a_process_name_is_matched_case_insensitively() {
        let rules = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "CS2.EXE")],
                "gaming",
            )],
            Some("default"),
        );
        let snapshot = snapshot(vec![window("cs2.exe", "Counter-Strike 2", "SDL_app")], None);
        assert_eq!(
            decide_from(&rules, &snapshot),
            Some(Decision {
                profile: "gaming".to_string(),
                rule_index: Some(0),
            })
        );
    }

    #[test]
    fn every_condition_is_tested_against_the_same_window() {
        let rules = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![
                    condition(Part::ProcessName, MatchMode::Contains, "cs2"),
                    condition(Part::Title, MatchMode::Contains, "counter"),
                ],
                "gaming",
            )],
            Some("default"),
        );
        // Two windows, one satisfying each half. "Process is cs2 AND a window
        // mentions Counter-Strike" would match this; the same-window rule must
        // not.
        let split = snapshot(
            vec![
                window("cs2.exe", "Loading...", "SDL_app"),
                window("chrome.exe", "Counter-Strike wiki", "Chrome_WidgetWin_1"),
            ],
            None,
        );
        assert_eq!(
            decide_from(&rules, &split),
            Some(Decision {
                profile: "default".to_string(),
                rule_index: None,
            }),
            "a rule was satisfied by two different windows"
        );

        // One window satisfying both.
        let together = snapshot(vec![window("cs2.exe", "Counter-Strike 2", "SDL_app")], None);
        assert_eq!(
            decide_from(&rules, &together).and_then(|decision| decision.rule_index),
            Some(0)
        );
    }

    #[test]
    fn an_any_rule_matches_when_one_condition_does() {
        let rules = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::Any,
                vec![
                    condition(Part::Title, MatchMode::Regex, "youtube"),
                    condition(Part::ClassName, MatchMode::Exact, "Chrome_WidgetWin_1"),
                ],
                "streaming",
            )],
            Some("default"),
        );
        let by_class = snapshot(
            vec![window("chrome.exe", "Settings", "Chrome_WidgetWin_1")],
            None,
        );
        assert_eq!(
            decide_from(&rules, &by_class).and_then(|decision| decision.rule_index),
            Some(0)
        );
        let neither = snapshot(vec![explorer()], None);
        assert!(decide_from(&rules, &neither).is_some_and(|d| d.rule_index.is_none()));
    }

    #[test]
    fn any_window_sees_past_the_foreground_and_foreground_does_not() {
        let background = vec![explorer(), window("cs2.exe", "Counter-Strike 2", "SDL_app")];
        let any = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                "gaming",
            )],
            Some("default"),
        );
        let snapshot = snapshot(background.clone(), Some(explorer()));
        assert_eq!(
            decide_from(&any, &snapshot).and_then(|decision| decision.rule_index),
            Some(0),
            "a game behind the foreground window should still match an any-window rule"
        );

        let focused = rules(
            vec![rule(
                Scope::Foreground,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                "gaming",
            )],
            Some("default"),
        );
        assert!(
            decide_from(&focused, &snapshot).is_some_and(|d| d.rule_index.is_none()),
            "a foreground rule matched a window that is not focused"
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let rules = rules(
            vec![
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                    "first",
                ),
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                    "second",
                ),
            ],
            Some("default"),
        );
        let snapshot = snapshot(vec![window("cs2.exe", "x", "y")], None);
        let decision = decide_from(&rules, &snapshot).expect("a match");
        assert_eq!(decision.profile, "first");
        assert_eq!(decision.rule_index, Some(0));
    }

    #[test]
    fn nothing_matching_falls_back_and_a_missing_fallback_is_inert() {
        let snapshot = snapshot(vec![explorer()], None);
        let with_fallback = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                "gaming",
            )],
            Some("default"),
        );
        assert_eq!(
            decide_from(&with_fallback, &snapshot),
            Some(Decision {
                profile: "default".to_string(),
                rule_index: None,
            })
        );

        let without_fallback = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                "gaming",
            )],
            None,
        );
        assert_eq!(decide_from(&without_fallback, &snapshot), None);
    }

    #[test]
    fn switched_off_rules_decide_nothing_even_with_a_match() {
        let mut file = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                "gaming",
            )],
            Some("default"),
        );
        file.enabled = false;
        let snapshot = snapshot(vec![window("cs2.exe", "x", "y")], None);
        assert_eq!(decide_from(&file, &snapshot), None);
    }

    #[test]
    fn enabled_with_no_usable_rules_is_inert_not_always_fallback() {
        // Nothing at all.
        let empty = rules(Vec::new(), Some("default"));
        assert_eq!(decide_from(&empty, &snapshot(vec![explorer()], None)), None);

        // Only a rule that cannot match: a half-typed condition must not
        // mass-apply the fallback.
        let broken = rules(
            vec![rule(
                Scope::AnyWindow,
                Combine::All,
                vec![condition(Part::ProcessName, MatchMode::Exact, "")],
                "gaming",
            )],
            Some("default"),
        );
        assert_eq!(
            decide_from(&broken, &snapshot(vec![explorer()], None)),
            None
        );
    }

    #[test]
    fn a_broken_rule_disables_itself_and_not_the_file() {
        let file = rules(
            vec![
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::Title, MatchMode::Regex, "(")],
                    "broken",
                ),
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                    "gaming",
                ),
            ],
            Some("default"),
        );
        let compiled = compile(&file);
        assert!(compiled.rules[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("regular expression")));
        assert!(compiled.rules[1].error.is_none());
        let snapshot = snapshot(vec![window("cs2.exe", "x", "y")], None);
        assert_eq!(
            decide(&compiled, &snapshot).and_then(|decision| decision.rule_index),
            Some(1)
        );
    }

    #[test]
    fn a_rule_missing_its_profile_or_a_value_is_reported() {
        let file = rules(
            vec![
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::ProcessName, MatchMode::Exact, "cs2.exe")],
                    "",
                ),
                rule(Scope::AnyWindow, Combine::All, Vec::new(), "gaming"),
                rule(
                    Scope::AnyWindow,
                    Combine::All,
                    vec![condition(Part::ProcessName, MatchMode::Exact, "  ")],
                    "gaming",
                ),
            ],
            Some("default"),
        );
        let compiled = compile(&file);
        assert_eq!(
            compiled.rules[0].error.as_deref(),
            Some("no profile to switch to")
        );
        assert_eq!(
            compiled.rules[1].error.as_deref(),
            Some("the rule has no conditions")
        );
        assert_eq!(
            compiled.rules[2].error.as_deref(),
            Some("condition 1 has an empty value")
        );
    }

    #[test]
    fn a_missing_part_is_rejected_rather_than_guessed_at() {
        let parsed: Result<AutoRules, _> =
            serde_json::from_str(r#"{"rules":[{"profile":"gaming","when":[{"value":"x"}]}]}"#);
        assert!(
            parsed.is_err(),
            "a condition with no part parsed, so a future file could silently mean processName"
        );
    }

    #[test]
    fn absent_fields_take_their_documented_defaults() {
        let parsed: AutoRules = serde_json::from_str(
            r#"{"enabled":true,"rules":[{"profile":"gaming","when":[{"part":"processName","value":"cs2.exe"}]}]}"#,
        )
        .expect("the minimal shape must parse");
        assert_eq!(parsed.fallback_profile, None);
        let rule = &parsed.rules[0];
        assert_eq!(rule.scope, Scope::AnyWindow);
        assert_eq!(rule.combine, Combine::All);
        assert_eq!(rule.name, "");
        assert_eq!(rule.when[0].matcher, MatchMode::Exact);
    }

    #[test]
    fn the_file_round_trips() {
        let file = rules(
            vec![Rule {
                name: "CS2".to_string(),
                scope: Scope::Foreground,
                combine: Combine::Any,
                when: vec![
                    condition(Part::Title, MatchMode::Regex, "(?i)counter.*strike"),
                    condition(Part::ClassName, MatchMode::Contains, "class"),
                ],
                profile: "gaming".to_string(),
            }],
            Some("default"),
        );
        let json = serde_json::to_string_pretty(&file).expect("serialize");
        let parsed: AutoRules = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, file);
    }

    #[test]
    fn the_first_tick_holds_and_the_second_applies() {
        let mut engine = Engine::default();
        assert_eq!(engine.step(Some("gaming")), Tick::Idle);
        assert_eq!(
            engine.step(Some("gaming")),
            Tick::Apply {
                profile: "gaming".to_string()
            }
        );
    }

    #[test]
    fn an_applied_profile_is_not_reapplied() {
        let mut engine = Engine::default();
        engine.step(Some("gaming"));
        engine.step(Some("gaming"));
        engine.mark_applied("gaming");
        for _ in 0..10 {
            assert_eq!(engine.step(Some("gaming")), Tick::Idle);
        }
        assert_eq!(engine.applied(), Some("gaming"));
    }

    #[test]
    fn a_failed_apply_is_retried_until_it_lands() {
        let mut engine = Engine::default();
        engine.step(Some("gaming"));
        assert_eq!(
            engine.step(Some("gaming")),
            Tick::Apply {
                profile: "gaming".to_string()
            }
        );
        // The push failed, so the caller did not mark it. The next tick must
        // ask again rather than treating the decision as done.
        assert_eq!(
            engine.step(Some("gaming")),
            Tick::Apply {
                profile: "gaming".to_string()
            }
        );
        engine.mark_applied("gaming");
        assert_eq!(engine.step(Some("gaming")), Tick::Idle);
    }

    #[test]
    fn a_flap_restarts_the_hold() {
        let mut engine = Engine::default();
        engine.step(Some("gaming"));
        assert_eq!(engine.step(Some("default")), Tick::Idle);
        assert_eq!(
            engine.step(Some("default")),
            Tick::Apply {
                profile: "default".to_string()
            },
            "the second tick of a new decision should apply it"
        );
    }

    #[test]
    fn switching_off_never_applies_and_resume_forces_one_push() {
        let mut engine = Engine::default();
        engine.step(Some("gaming"));
        assert_eq!(
            engine.step(Some("gaming")),
            Tick::Apply {
                profile: "gaming".to_string()
            }
        );
        engine.mark_applied("gaming");

        // Automation switched off: nothing to do, and the record survives.
        assert_eq!(engine.step(None), Tick::Idle);
        assert_eq!(engine.applied(), Some("gaming"));

        // The Config window was opened and closed. Even if it chose the same
        // profile, the next settled decision must go out again, because the
        // engine cannot know what happened while it was not the authority.
        engine.resume();
        assert_eq!(engine.applied(), None);
        engine.step(Some("gaming"));
        assert_eq!(
            engine.step(Some("gaming")),
            Tick::Apply {
                profile: "gaming".to_string()
            }
        );
    }
}
