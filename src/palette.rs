use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteEntry {
    pub name: &'static str,
    pub color: Color,
    pub source_unused: bool,
}

macro_rules! define_palette {
    ($(($constant:ident, $name:literal, $color:expr, $source_unused:literal)),+ $(,)?) => {
        $(pub const $constant: Color = $color;)+

        pub const ALL: &[PaletteEntry] = &[
            $(PaletteEntry {
                name: $name,
                color: $constant,
                source_unused: $source_unused,
            }),+
        ];
    };
}

define_palette!(
    (AUTO_ACCEPT, "autoAccept", Color::Rgb(175, 135, 255), false),
    (BASH_BORDER, "bashBorder", Color::Rgb(253, 93, 177), false),
    (CLAUDE, "claude", Color::Rgb(215, 119, 87), false),
    (
        CLAUDE_SHIMMER,
        "claudeShimmer",
        Color::Rgb(235, 159, 127),
        false
    ),
    (
        CLAUDE_BLUE_FOR_SYSTEM_SPINNER,
        "claudeBlue_FOR_SYSTEM_SPINNER",
        Color::Rgb(147, 165, 255),
        false
    ),
    (
        CLAUDE_BLUE_SHIMMER_FOR_SYSTEM_SPINNER,
        "claudeBlueShimmer_FOR_SYSTEM_SPINNER",
        Color::Rgb(177, 195, 255),
        false
    ),
    (PERMISSION, "permission", Color::Rgb(177, 185, 249), false),
    (
        PERMISSION_SHIMMER,
        "permissionShimmer",
        Color::Rgb(207, 215, 255),
        true
    ),
    (PLAN_MODE, "planMode", Color::Rgb(72, 150, 140), false),
    (IDE, "ide", Color::Rgb(71, 130, 200), false),
    (
        PROMPT_BORDER,
        "promptBorder",
        Color::Rgb(136, 136, 136),
        false
    ),
    (
        PROMPT_BORDER_SHIMMER,
        "promptBorderShimmer",
        Color::Rgb(166, 166, 166),
        true
    ),
    (TEXT, "text", Color::Rgb(255, 255, 255), false),
    (INVERSE_TEXT, "inverseText", Color::Rgb(0, 0, 0), false),
    (INACTIVE, "inactive", Color::Rgb(153, 153, 153), false),
    (
        INACTIVE_SHIMMER,
        "inactiveShimmer",
        Color::Rgb(193, 193, 193),
        true
    ),
    (SUBTLE, "subtle", Color::Rgb(80, 80, 80), false),
    (SUGGESTION, "suggestion", Color::Rgb(177, 185, 249), false),
    (REMEMBER, "remember", Color::Rgb(177, 185, 249), false),
    (BACKGROUND, "background", Color::Rgb(0, 204, 204), false),
    (SUCCESS, "success", Color::Rgb(78, 186, 101), false),
    (ERROR, "error", Color::Rgb(255, 107, 128), false),
    (WARNING, "warning", Color::Rgb(255, 193, 7), false),
    (MERGED, "merged", Color::Rgb(175, 135, 255), false),
    (
        WARNING_SHIMMER,
        "warningShimmer",
        Color::Rgb(255, 223, 57),
        true
    ),
    (DIFF_ADDED, "diffAdded", Color::Rgb(34, 92, 43), false),
    (DIFF_REMOVED, "diffRemoved", Color::Rgb(122, 41, 54), false),
    (
        DIFF_ADDED_DIMMED,
        "diffAddedDimmed",
        Color::Rgb(71, 88, 74),
        false
    ),
    (
        DIFF_REMOVED_DIMMED,
        "diffRemovedDimmed",
        Color::Rgb(105, 72, 77),
        false
    ),
    (
        DIFF_ADDED_WORD,
        "diffAddedWord",
        Color::Rgb(56, 166, 96),
        false
    ),
    (
        DIFF_REMOVED_WORD,
        "diffRemovedWord",
        Color::Rgb(179, 89, 107),
        false
    ),
    (
        RED_FOR_SUBAGENTS_ONLY,
        "red_FOR_SUBAGENTS_ONLY",
        Color::Rgb(220, 38, 38),
        false
    ),
    (
        BLUE_FOR_SUBAGENTS_ONLY,
        "blue_FOR_SUBAGENTS_ONLY",
        Color::Rgb(37, 99, 235),
        false
    ),
    (
        GREEN_FOR_SUBAGENTS_ONLY,
        "green_FOR_SUBAGENTS_ONLY",
        Color::Rgb(22, 163, 74),
        false
    ),
    (
        YELLOW_FOR_SUBAGENTS_ONLY,
        "yellow_FOR_SUBAGENTS_ONLY",
        Color::Rgb(202, 138, 4),
        false
    ),
    (
        PURPLE_FOR_SUBAGENTS_ONLY,
        "purple_FOR_SUBAGENTS_ONLY",
        Color::Rgb(147, 51, 234),
        false
    ),
    (
        ORANGE_FOR_SUBAGENTS_ONLY,
        "orange_FOR_SUBAGENTS_ONLY",
        Color::Rgb(234, 88, 12),
        false
    ),
    (
        PINK_FOR_SUBAGENTS_ONLY,
        "pink_FOR_SUBAGENTS_ONLY",
        Color::Rgb(219, 39, 119),
        false
    ),
    (
        CYAN_FOR_SUBAGENTS_ONLY,
        "cyan_FOR_SUBAGENTS_ONLY",
        Color::Rgb(8, 145, 178),
        false
    ),
    (
        PROFESSIONAL_BLUE,
        "professionalBlue",
        Color::Rgb(106, 155, 204),
        false
    ),
    (
        CHROME_YELLOW,
        "chromeYellow",
        Color::Rgb(251, 188, 4),
        false
    ),
    (CLAWD_BODY, "clawd_body", Color::Rgb(215, 119, 87), false),
    (
        CLAWD_BACKGROUND,
        "clawd_background",
        Color::Rgb(0, 0, 0),
        false
    ),
    (
        USER_MESSAGE_BACKGROUND,
        "userMessageBackground",
        Color::Rgb(55, 55, 55),
        false
    ),
    (
        USER_MESSAGE_BACKGROUND_HOVER,
        "userMessageBackgroundHover",
        Color::Rgb(70, 70, 70),
        false
    ),
    (
        MESSAGE_ACTIONS_BACKGROUND,
        "messageActionsBackground",
        Color::Rgb(44, 50, 62),
        false
    ),
    (SELECTION_BG, "selectionBg", Color::Rgb(38, 79, 120), false),
    (
        BASH_MESSAGE_BACKGROUND_COLOR,
        "bashMessageBackgroundColor",
        Color::Rgb(65, 60, 65),
        false
    ),
    (
        MEMORY_BACKGROUND_COLOR,
        "memoryBackgroundColor",
        Color::Rgb(55, 65, 70),
        false
    ),
    (
        RATE_LIMIT_FILL,
        "rate_limit_fill",
        Color::Rgb(177, 185, 249),
        false
    ),
    (
        RATE_LIMIT_EMPTY,
        "rate_limit_empty",
        Color::Rgb(80, 83, 112),
        false
    ),
    (FAST_MODE, "fastMode", Color::Rgb(255, 120, 20), false),
    (
        FAST_MODE_SHIMMER,
        "fastModeShimmer",
        Color::Rgb(255, 165, 70),
        true
    ),
    (
        BRIEF_LABEL_YOU,
        "briefLabelYou",
        Color::Rgb(122, 180, 232),
        false
    ),
    (
        BRIEF_LABEL_CLAUDE,
        "briefLabelClaude",
        Color::Rgb(215, 119, 87),
        false
    ),
    (RAINBOW_RED, "rainbow_red", Color::Rgb(235, 95, 87), false),
    (
        RAINBOW_ORANGE,
        "rainbow_orange",
        Color::Rgb(245, 139, 87),
        false
    ),
    (
        RAINBOW_YELLOW,
        "rainbow_yellow",
        Color::Rgb(250, 195, 95),
        false
    ),
    (
        RAINBOW_GREEN,
        "rainbow_green",
        Color::Rgb(145, 200, 130),
        false
    ),
    (
        RAINBOW_BLUE,
        "rainbow_blue",
        Color::Rgb(130, 170, 220),
        false
    ),
    (
        RAINBOW_INDIGO,
        "rainbow_indigo",
        Color::Rgb(155, 130, 200),
        false
    ),
    (
        RAINBOW_VIOLET,
        "rainbow_violet",
        Color::Rgb(200, 130, 180),
        false
    ),
    (
        RAINBOW_RED_SHIMMER,
        "rainbow_red_shimmer",
        Color::Rgb(250, 155, 147),
        false
    ),
    (
        RAINBOW_ORANGE_SHIMMER,
        "rainbow_orange_shimmer",
        Color::Rgb(255, 185, 137),
        false
    ),
    (
        RAINBOW_YELLOW_SHIMMER,
        "rainbow_yellow_shimmer",
        Color::Rgb(255, 225, 155),
        false
    ),
    (
        RAINBOW_GREEN_SHIMMER,
        "rainbow_green_shimmer",
        Color::Rgb(185, 230, 180),
        false
    ),
    (
        RAINBOW_BLUE_SHIMMER,
        "rainbow_blue_shimmer",
        Color::Rgb(180, 205, 240),
        false
    ),
    (
        RAINBOW_INDIGO_SHIMMER,
        "rainbow_indigo_shimmer",
        Color::Rgb(195, 180, 230),
        false
    ),
    (
        RAINBOW_VIOLET_SHIMMER,
        "rainbow_violet_shimmer",
        Color::Rgb(230, 180, 210),
        false
    ),
);

pub fn color(name: &str) -> Option<Color> {
    ALL.iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.color)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use ratatui::style::Color;

    use super::{color, PaletteEntry, ALL};

    const EXPECTED: &[PaletteEntry] = &[
        PaletteEntry {
            name: "autoAccept",
            color: Color::Rgb(175, 135, 255),
            source_unused: false,
        },
        PaletteEntry {
            name: "bashBorder",
            color: Color::Rgb(253, 93, 177),
            source_unused: false,
        },
        PaletteEntry {
            name: "claude",
            color: Color::Rgb(215, 119, 87),
            source_unused: false,
        },
        PaletteEntry {
            name: "claudeShimmer",
            color: Color::Rgb(235, 159, 127),
            source_unused: false,
        },
        PaletteEntry {
            name: "claudeBlue_FOR_SYSTEM_SPINNER",
            color: Color::Rgb(147, 165, 255),
            source_unused: false,
        },
        PaletteEntry {
            name: "claudeBlueShimmer_FOR_SYSTEM_SPINNER",
            color: Color::Rgb(177, 195, 255),
            source_unused: false,
        },
        PaletteEntry {
            name: "permission",
            color: Color::Rgb(177, 185, 249),
            source_unused: false,
        },
        PaletteEntry {
            name: "permissionShimmer",
            color: Color::Rgb(207, 215, 255),
            source_unused: true,
        },
        PaletteEntry {
            name: "planMode",
            color: Color::Rgb(72, 150, 140),
            source_unused: false,
        },
        PaletteEntry {
            name: "ide",
            color: Color::Rgb(71, 130, 200),
            source_unused: false,
        },
        PaletteEntry {
            name: "promptBorder",
            color: Color::Rgb(136, 136, 136),
            source_unused: false,
        },
        PaletteEntry {
            name: "promptBorderShimmer",
            color: Color::Rgb(166, 166, 166),
            source_unused: true,
        },
        PaletteEntry {
            name: "text",
            color: Color::Rgb(255, 255, 255),
            source_unused: false,
        },
        PaletteEntry {
            name: "inverseText",
            color: Color::Rgb(0, 0, 0),
            source_unused: false,
        },
        PaletteEntry {
            name: "inactive",
            color: Color::Rgb(153, 153, 153),
            source_unused: false,
        },
        PaletteEntry {
            name: "inactiveShimmer",
            color: Color::Rgb(193, 193, 193),
            source_unused: true,
        },
        PaletteEntry {
            name: "subtle",
            color: Color::Rgb(80, 80, 80),
            source_unused: false,
        },
        PaletteEntry {
            name: "suggestion",
            color: Color::Rgb(177, 185, 249),
            source_unused: false,
        },
        PaletteEntry {
            name: "remember",
            color: Color::Rgb(177, 185, 249),
            source_unused: false,
        },
        PaletteEntry {
            name: "background",
            color: Color::Rgb(0, 204, 204),
            source_unused: false,
        },
        PaletteEntry {
            name: "success",
            color: Color::Rgb(78, 186, 101),
            source_unused: false,
        },
        PaletteEntry {
            name: "error",
            color: Color::Rgb(255, 107, 128),
            source_unused: false,
        },
        PaletteEntry {
            name: "warning",
            color: Color::Rgb(255, 193, 7),
            source_unused: false,
        },
        PaletteEntry {
            name: "merged",
            color: Color::Rgb(175, 135, 255),
            source_unused: false,
        },
        PaletteEntry {
            name: "warningShimmer",
            color: Color::Rgb(255, 223, 57),
            source_unused: true,
        },
        PaletteEntry {
            name: "diffAdded",
            color: Color::Rgb(34, 92, 43),
            source_unused: false,
        },
        PaletteEntry {
            name: "diffRemoved",
            color: Color::Rgb(122, 41, 54),
            source_unused: false,
        },
        PaletteEntry {
            name: "diffAddedDimmed",
            color: Color::Rgb(71, 88, 74),
            source_unused: false,
        },
        PaletteEntry {
            name: "diffRemovedDimmed",
            color: Color::Rgb(105, 72, 77),
            source_unused: false,
        },
        PaletteEntry {
            name: "diffAddedWord",
            color: Color::Rgb(56, 166, 96),
            source_unused: false,
        },
        PaletteEntry {
            name: "diffRemovedWord",
            color: Color::Rgb(179, 89, 107),
            source_unused: false,
        },
        PaletteEntry {
            name: "red_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(220, 38, 38),
            source_unused: false,
        },
        PaletteEntry {
            name: "blue_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(37, 99, 235),
            source_unused: false,
        },
        PaletteEntry {
            name: "green_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(22, 163, 74),
            source_unused: false,
        },
        PaletteEntry {
            name: "yellow_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(202, 138, 4),
            source_unused: false,
        },
        PaletteEntry {
            name: "purple_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(147, 51, 234),
            source_unused: false,
        },
        PaletteEntry {
            name: "orange_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(234, 88, 12),
            source_unused: false,
        },
        PaletteEntry {
            name: "pink_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(219, 39, 119),
            source_unused: false,
        },
        PaletteEntry {
            name: "cyan_FOR_SUBAGENTS_ONLY",
            color: Color::Rgb(8, 145, 178),
            source_unused: false,
        },
        PaletteEntry {
            name: "professionalBlue",
            color: Color::Rgb(106, 155, 204),
            source_unused: false,
        },
        PaletteEntry {
            name: "chromeYellow",
            color: Color::Rgb(251, 188, 4),
            source_unused: false,
        },
        PaletteEntry {
            name: "clawd_body",
            color: Color::Rgb(215, 119, 87),
            source_unused: false,
        },
        PaletteEntry {
            name: "clawd_background",
            color: Color::Rgb(0, 0, 0),
            source_unused: false,
        },
        PaletteEntry {
            name: "userMessageBackground",
            color: Color::Rgb(55, 55, 55),
            source_unused: false,
        },
        PaletteEntry {
            name: "userMessageBackgroundHover",
            color: Color::Rgb(70, 70, 70),
            source_unused: false,
        },
        PaletteEntry {
            name: "messageActionsBackground",
            color: Color::Rgb(44, 50, 62),
            source_unused: false,
        },
        PaletteEntry {
            name: "selectionBg",
            color: Color::Rgb(38, 79, 120),
            source_unused: false,
        },
        PaletteEntry {
            name: "bashMessageBackgroundColor",
            color: Color::Rgb(65, 60, 65),
            source_unused: false,
        },
        PaletteEntry {
            name: "memoryBackgroundColor",
            color: Color::Rgb(55, 65, 70),
            source_unused: false,
        },
        PaletteEntry {
            name: "rate_limit_fill",
            color: Color::Rgb(177, 185, 249),
            source_unused: false,
        },
        PaletteEntry {
            name: "rate_limit_empty",
            color: Color::Rgb(80, 83, 112),
            source_unused: false,
        },
        PaletteEntry {
            name: "fastMode",
            color: Color::Rgb(255, 120, 20),
            source_unused: false,
        },
        PaletteEntry {
            name: "fastModeShimmer",
            color: Color::Rgb(255, 165, 70),
            source_unused: true,
        },
        PaletteEntry {
            name: "briefLabelYou",
            color: Color::Rgb(122, 180, 232),
            source_unused: false,
        },
        PaletteEntry {
            name: "briefLabelClaude",
            color: Color::Rgb(215, 119, 87),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_red",
            color: Color::Rgb(235, 95, 87),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_orange",
            color: Color::Rgb(245, 139, 87),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_yellow",
            color: Color::Rgb(250, 195, 95),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_green",
            color: Color::Rgb(145, 200, 130),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_blue",
            color: Color::Rgb(130, 170, 220),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_indigo",
            color: Color::Rgb(155, 130, 200),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_violet",
            color: Color::Rgb(200, 130, 180),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_red_shimmer",
            color: Color::Rgb(250, 155, 147),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_orange_shimmer",
            color: Color::Rgb(255, 185, 137),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_yellow_shimmer",
            color: Color::Rgb(255, 225, 155),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_green_shimmer",
            color: Color::Rgb(185, 230, 180),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_blue_shimmer",
            color: Color::Rgb(180, 205, 240),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_indigo_shimmer",
            color: Color::Rgb(195, 180, 230),
            source_unused: false,
        },
        PaletteEntry {
            name: "rainbow_violet_shimmer",
            color: Color::Rgb(230, 180, 210),
            source_unused: false,
        },
    ];

    #[test]
    fn every_dark_palette_key_resolves_to_its_exact_value() {
        assert_eq!(ALL.len(), EXPECTED.len());

        for (actual, expected) in ALL.iter().zip(EXPECTED) {
            assert_eq!(actual, expected);
            assert_eq!(color(expected.name), Some(expected.color));
        }
    }

    #[test]
    fn palette_keys_are_unique_and_complete_against_the_expected_table() {
        let names = ALL.iter().map(|entry| entry.name).collect::<Vec<_>>();
        let unique_names = names.iter().collect::<HashSet<_>>();
        let expected_names = EXPECTED.iter().map(|entry| entry.name).collect::<Vec<_>>();

        assert_eq!(names.len(), 69);
        assert_eq!(unique_names.len(), names.len());
        assert_eq!(names, expected_names);
    }
}
