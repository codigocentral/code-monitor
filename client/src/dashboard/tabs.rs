//! Dashboard tab definitions
//!
//! One list drives everything about tabs: their order, titles, hotkeys and
//! wrap-around. Adding a tab means adding a variant here and rendering it —
//! not hunting down a hardcoded count, a modulo and a set of key bindings.

/// A tab in the dashboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Services,
    Processes,
    Network,
    Containers,
    Postgres,
    MariaDB,
    Systemd,
    Tls,
}

impl Tab {
    /// Every tab, in display order. The single source of truth.
    pub const ALL: &'static [Tab] = &[
        Tab::Overview,
        Tab::Services,
        Tab::Processes,
        Tab::Network,
        Tab::Containers,
        Tab::Postgres,
        Tab::MariaDB,
        Tab::Systemd,
        Tab::Tls,
    ];

    /// Title shown in the tab bar
    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "󰍹 Overview",
            Tab::Services => "󰒍 Services",
            Tab::Processes => "󰓁 Processes",
            Tab::Network => "󰛳 Network",
            Tab::Containers => "󰡨 Containers",
            Tab::Postgres => "🐘 Postgres",
            Tab::MariaDB => "🗄️ MariaDB",
            Tab::Systemd => "⚙️ Systemd",
            Tab::Tls => "󰌾 TLS",
        }
    }

    /// Position in the tab bar
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|t| *t == self)
            .expect("every tab is listed in ALL")
    }

    /// Tab at the given position, if any
    pub fn from_index(index: usize) -> Option<Tab> {
        Self::ALL.get(index).copied()
    }

    /// Tab bound to a number key, where `1` selects the first tab
    ///
    /// Only the first nine tabs are reachable this way, since there are no
    /// further single digits. Tab and Shift-Tab reach the rest.
    pub fn from_hotkey(key: char) -> Option<Tab> {
        let digit = key.to_digit(10)? as usize;
        if digit == 0 {
            return None;
        }
        Self::from_index(digit - 1)
    }

    /// Next tab, wrapping around at the end
    pub fn next(self) -> Tab {
        let next = (self.index() + 1) % Self::ALL.len();
        Self::from_index(next).unwrap_or(Tab::Overview)
    }

    /// Previous tab, wrapping around at the start
    pub fn previous(self) -> Tab {
        let index = self.index();
        let previous = if index == 0 {
            Self::ALL.len() - 1
        } else {
            index - 1
        };
        Self::from_index(previous).unwrap_or(Tab::Overview)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_round_trips_for_every_tab() {
        for tab in Tab::ALL {
            assert_eq!(Tab::from_index(tab.index()), Some(*tab));
        }
    }

    #[test]
    fn test_indices_are_contiguous_and_ordered() {
        for (position, tab) in Tab::ALL.iter().enumerate() {
            assert_eq!(tab.index(), position);
        }
    }

    #[test]
    fn test_from_index_out_of_range() {
        assert_eq!(Tab::from_index(Tab::ALL.len()), None);
        assert_eq!(Tab::from_index(999), None);
    }

    #[test]
    fn test_next_wraps_around() {
        assert_eq!(Tab::Overview.next(), Tab::Services);
        assert_eq!(Tab::ALL[Tab::ALL.len() - 1].next(), Tab::Overview);
    }

    #[test]
    fn test_previous_wraps_around() {
        assert_eq!(Tab::Services.previous(), Tab::Overview);
        assert_eq!(
            Tab::Overview.previous(),
            Tab::ALL[Tab::ALL.len() - 1],
            "wrapping backwards must land on the last tab, whatever it is"
        );
    }

    #[test]
    fn test_next_then_previous_is_identity() {
        for tab in Tab::ALL {
            assert_eq!(tab.next().previous(), *tab);
        }
    }

    #[test]
    fn test_hotkeys_are_one_based() {
        assert_eq!(Tab::from_hotkey('1'), Some(Tab::Overview));
        assert_eq!(Tab::from_hotkey('8'), Some(Tab::Systemd));
        assert_eq!(Tab::from_hotkey('9'), Some(Tab::Tls));
    }

    #[test]
    fn test_hotkey_zero_and_non_digit() {
        assert_eq!(Tab::from_hotkey('0'), None);
        assert_eq!(Tab::from_hotkey('a'), None);
        assert_eq!(Tab::from_hotkey(' '), None);
    }

    #[test]
    fn test_hotkey_past_the_last_tab() {
        // Only meaningful while there are fewer than nine tabs; past that the
        // digits run out and Tab/Shift-Tab are the only way through.
        if let Some(digit) = char::from_digit(Tab::ALL.len() as u32 + 1, 10) {
            assert_eq!(Tab::from_hotkey(digit), None);
        }
    }

    #[test]
    fn test_hotkeys_cover_as_many_tabs_as_digits_allow() {
        let reachable = Tab::ALL.len().min(9);
        for position in 0..reachable {
            let digit = char::from_digit(position as u32 + 1, 10).unwrap();
            assert_eq!(Tab::from_hotkey(digit), Some(Tab::ALL[position]));
        }
    }

    #[test]
    fn test_every_tab_has_a_title() {
        for tab in Tab::ALL {
            assert!(!tab.title().is_empty());
        }
    }

    #[test]
    fn test_titles_are_unique() {
        let mut titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
        let total = titles.len();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(total, titles.len());
    }

    #[test]
    fn test_default_is_overview() {
        assert_eq!(Tab::default(), Tab::Overview);
    }
}
