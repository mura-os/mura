//! The menu state machine (specs/recovery-menu.md §3), shared by the panel and shell frontends.
//! Pure: inputs in, at most one action out; no I/O, so it is unit-tested here.

use crate::actions::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Next,
    Prev,
    Select,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    Confirm,
    Details,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    TryAgain,
    FactoryReset,
    SwitchSlot,
    PowerOff,
    ShowDetails,
}

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::TryAgain => "Try again (reboot)",
            Item::FactoryReset => "Factory reset\u{2026}",
            Item::SwitchSlot => "Switch system slot",
            Item::PowerOff => "Power off",
            Item::ShowDetails => "Show details",
        }
    }
}

pub const CONFIRM_HEADER: &str =
    "Erase everything this headset has stored? Accounts, settings, Wi-Fi, pairings. The headset gets a new SSH identity. THIS CANNOT BE UNDONE.";
pub const CONFIRM_ITEMS: [&str; 2] = ["Cancel", "Erase everything"];

#[derive(Debug, Clone)]
pub struct Menu {
    pub items: Vec<Item>,
    pub selected: usize,
    pub screen: Screen,
    /// Confirm screen selection: 0 = Cancel (default), 1 = Erase everything.
    pub confirm_selected: usize,
}

impl Menu {
    pub fn new(has_switch_slot: bool) -> Self {
        let mut items = vec![Item::TryAgain, Item::FactoryReset];
        if has_switch_slot {
            items.push(Item::SwitchSlot);
        }
        items.push(Item::PowerOff);
        items.push(Item::ShowDetails);
        Menu { items, selected: 0, screen: Screen::Main, confirm_selected: 0 }
    }

    /// One input, at most one action (§3: "at most one action per input").
    pub fn on(&mut self, input: Input) -> Option<Action> {
        match self.screen {
            Screen::Main => match input {
                Input::Next => {
                    self.selected = (self.selected + 1) % self.items.len();
                    None
                }
                Input::Prev => {
                    self.selected = (self.selected + self.items.len() - 1) % self.items.len();
                    None
                }
                Input::Back => None,
                Input::Select => match self.items[self.selected] {
                    Item::TryAgain => Some(Action::Reboot),
                    Item::FactoryReset => {
                        // never acts directly: open Confirm with Cancel selected
                        self.screen = Screen::Confirm;
                        self.confirm_selected = 0;
                        None
                    }
                    Item::SwitchSlot => Some(Action::SwitchSlot),
                    Item::PowerOff => Some(Action::PowerOff),
                    Item::ShowDetails => {
                        self.screen = Screen::Details;
                        None
                    }
                },
            },
            Screen::Confirm => match input {
                Input::Next | Input::Prev => {
                    self.confirm_selected ^= 1;
                    None
                }
                Input::Back => {
                    self.screen = Screen::Main;
                    None
                }
                Input::Select => {
                    let erase = self.confirm_selected == 1;
                    self.screen = Screen::Main;
                    self.confirm_selected = 0;
                    if erase { Some(Action::FactoryReset) } else { None }
                }
            },
            Screen::Details => {
                // any key returns
                self.screen = Screen::Main;
                None
            }
        }
    }

    /// The screen as text (§5): title, then items with `> ` on the selected one. The panel
    /// frontend sends this as one plymouth message; the shell prints it.
    pub fn render(&self, details: &str) -> String {
        match self.screen {
            Screen::Main => {
                let mut s = String::from("Mura recovery\n");
                for (i, it) in self.items.iter().enumerate() {
                    s.push_str(if i == self.selected { "> " } else { "  " });
                    s.push_str(it.label());
                    s.push('\n');
                }
                s
            }
            Screen::Confirm => {
                let mut s = format!("{CONFIRM_HEADER}\n");
                for (i, it) in CONFIRM_ITEMS.iter().enumerate() {
                    s.push_str(if i == self.confirm_selected { "> " } else { "  " });
                    s.push_str(it);
                    s.push('\n');
                }
                s
            }
            Screen::Details => format!("{details}\n(any key returns)\n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_again_is_default_and_reboots() {
        let mut m = Menu::new(false);
        assert_eq!(m.items[m.selected], Item::TryAgain);
        assert_eq!(m.on(Input::Select), Some(Action::Reboot));
    }

    #[test]
    fn factory_reset_opens_confirm_with_cancel_default() {
        let mut m = Menu::new(false);
        assert_eq!(m.on(Input::Next), None);
        assert_eq!(m.items[m.selected], Item::FactoryReset);
        assert_eq!(m.on(Input::Select), None); // never acts directly
        assert_eq!(m.screen, Screen::Confirm);
        assert_eq!(m.confirm_selected, 0);
        // the same key that opened Confirm does nothing destructive there
        assert_eq!(m.on(Input::Select), None);
        assert_eq!(m.screen, Screen::Main);
        assert_eq!(m.items[m.selected], Item::FactoryReset);
    }

    #[test]
    fn erase_needs_a_move_and_a_select_on_confirm() {
        let mut m = Menu::new(false);
        m.on(Input::Next);
        m.on(Input::Select);
        assert_eq!(m.on(Input::Next), None);
        assert_eq!(m.confirm_selected, 1);
        assert_eq!(m.on(Input::Select), Some(Action::FactoryReset));
        assert_eq!(m.screen, Screen::Main);
    }

    #[test]
    fn back_leaves_confirm_without_acting() {
        let mut m = Menu::new(false);
        m.on(Input::Next);
        m.on(Input::Select);
        m.on(Input::Next); // Erase everything highlighted
        assert_eq!(m.on(Input::Back), None);
        assert_eq!(m.screen, Screen::Main);
        // re-entering Confirm resets to Cancel
        assert_eq!(m.on(Input::Select), None);
        assert_eq!(m.confirm_selected, 0);
    }

    #[test]
    fn one_input_is_one_step_whatever_the_repeat() {
        // the frontend registers a held key once (release); the machine has no repeat of its own
        let mut m = Menu::new(true);
        m.on(Input::Next);
        assert_eq!(m.items[m.selected], Item::FactoryReset);
        m.on(Input::Prev);
        assert_eq!(m.items[m.selected], Item::TryAgain);
        m.on(Input::Prev); // wraps
        assert_eq!(m.items[m.selected], Item::ShowDetails);
    }

    #[test]
    fn switch_slot_only_when_configured() {
        assert!(!Menu::new(false).items.contains(&Item::SwitchSlot));
        assert!(Menu::new(true).items.contains(&Item::SwitchSlot));
    }

    #[test]
    fn render_marks_selection_and_fits_plymouth() {
        let m = Menu::new(true);
        let r = m.render("");
        assert!(r.starts_with("Mura recovery\n> Try again (reboot)\n  Factory reset"), "{r}");
        assert!(r.len() <= 200, "main screen must fit one plymouth message: {}", r.len());
        let mut c = Menu::new(false);
        c.on(Input::Next);
        c.on(Input::Select);
        assert!(c.render("").contains("> Cancel"));
        assert!(c.render("").len() <= 200);
    }
}
