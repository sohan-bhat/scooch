#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gnome,
    Kde,
    Xfce,
    Cinnamon,
    Mate,
}

pub const UNKNOWN: &str = "Scooch does not know this desktop yet, so it cannot see where your icons sit.";

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Gnome => "GNOME",
            Kind::Kde => "KDE Plasma",
            Kind::Xfce => "Xfce",
            Kind::Cinnamon => "Cinnamon",
            Kind::Mate => "MATE",
        }
    }
}

pub fn pick(value: &str) -> Option<Kind> {
    value.split(':').map(str::trim).find_map(|part| {
        let part = part.to_ascii_lowercase();
        match part.as_str() {
            "gnome" | "unity" | "ubuntu" => Some(Kind::Gnome),
            "x-cinnamon" | "cinnamon" => Some(Kind::Cinnamon),
            "mate" => Some(Kind::Mate),
            "xfce" => Some(Kind::Xfce),
            "kde" => Some(Kind::Kde),
            _ => None,
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session {
    Wayland,
    X11,
}

pub fn session(wayland_display: Option<&str>) -> Session {
    match wayland_display {
        Some(name) if !name.is_empty() => Session::Wayland,
        _ => Session::X11,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_desktop_name_picks_its_backend() {
        assert_eq!(pick("GNOME"), Some(Kind::Gnome));
        assert_eq!(pick("ubuntu:GNOME"), Some(Kind::Gnome));
        assert_eq!(pick("Unity"), Some(Kind::Gnome));
        assert_eq!(pick("X-Cinnamon"), Some(Kind::Cinnamon));
        assert_eq!(pick("cinnamon"), Some(Kind::Cinnamon));
        assert_eq!(pick("MATE"), Some(Kind::Mate));
        assert_eq!(pick("XFCE"), Some(Kind::Xfce));
        assert_eq!(pick("KDE"), Some(Kind::Kde));
    }

    #[test]
    fn the_first_known_part_wins_and_case_is_ignored() {
        assert_eq!(pick("Budgie:gnome"), Some(Kind::Gnome));
        assert_eq!(pick("pop:kde"), Some(Kind::Kde));
        assert_eq!(pick(" xfce "), Some(Kind::Xfce));
    }

    #[test]
    fn unknown_desktops_are_none() {
        assert_eq!(pick("sway"), None);
        assert_eq!(pick(""), None);
        assert!(!UNKNOWN.contains([':', ';', '(', ')']));
    }

    #[test]
    fn a_wayland_display_means_wayland() {
        assert_eq!(session(Some("wayland-0")), Session::Wayland);
        assert_eq!(session(Some("")), Session::X11);
        assert_eq!(session(None), Session::X11);
    }
}
