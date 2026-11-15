//! Utilities to parse Minecraft RCON command output.

/// Parse the output of Minecraft's `list` or `list uuids` command.
///
/// Typical Vanilla / Paper / Fabric output formats:
/// - "There are 0 of a max of 20 players online:"
/// - "There are 2 of a max of 20 players online: Steve, Alex"
/// - "There are 2/20 players online: Steve, Alex"
/// - "Connected players: Steve, Alex"
///
/// Returns a list of trimmed player usernames.
pub fn parse_player_list(output: &str) -> Vec<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Usually the list is after a colon ':'
    let names_part = if let Some((_, after_colon)) = trimmed.split_once(':') {
        after_colon.trim()
    } else {
        // Fallback: if no colon, maybe comma-separated list directly
        trimmed
    };

    if names_part.is_empty() {
        return Vec::new();
    }

    names_part
        .split(',')
        .map(|name| {
            // Strip any bracketed UUID or prefixes if present, e.g. "Steve (uuid)"
            let clean = name.trim();
            if let Some((uname, _)) = clean.split_once(' ') {
                uname.trim().to_string()
            } else {
                clean.to_string()
            }
        })
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_lists() {
        assert_eq!(
            parse_player_list("There are 0 of a max of 20 players online:"),
            Vec::<String>::new()
        );
        assert_eq!(
            parse_player_list("There are 0/20 players online: "),
            Vec::<String>::new()
        );
        assert_eq!(parse_player_list(""), Vec::<String>::new());
    }

    #[test]
    fn parses_active_players() {
        assert_eq!(
            parse_player_list("There are 2 of a max of 20 players online: Steve, Alex"),
            vec!["Steve", "Alex"]
        );
        assert_eq!(
            parse_player_list("Connected players: Steve, Alex, Notch_123"),
            vec!["Steve", "Alex", "Notch_123"]
        );
        assert_eq!(
            parse_player_list("There are 1 players online: Player_One (uuid-here)"),
            vec!["Player_One"]
        );
    }
}
