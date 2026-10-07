//! Block F3 and the runner facts' `exe-lookup`: which file `std::process::Command` starts
//! for a bare program name. W17 decides from it whether Pitboard may ever start a tool by a
//! bare name; the question is whether a `claude.exe` beside `pitboard.exe` wins over the one
//! on `PATH`.
//!
//! The measurement sets up stand-ins of one probe-named program in four places (beside the
//! program that does the lookup, in a folder on the parent's `PATH`, in a folder on a
//! changed child `PATH`, and in the working folder), plus a `.cmd` of the same name, and
//! reports which one ran in each case. Each stand-in is a copy of the probe that answers
//! with the place it sits in. Nothing here assumes the order; the cases are what is
//! measured.

use crate::PROBE_PREFIX;

/// Whether `name` is a bare program name the probe may make stand-ins of: a
/// `pitboard-probe-*` name of letters, digits and dashes, with no extension, separator or
/// dot, so no stand-in can ever take the name of a real program.
pub fn is_probe_name(name: &str) -> bool {
    name.len() > PROBE_PREFIX.len()
        && name.len() <= 64
        && name.starts_with(PROBE_PREFIX)
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether `path` is one the hidden `exe-lookup-host` may put first on its child's `PATH`:
/// an absolute path, with no `..`, to the probe's own child-path folder.
pub fn is_child_path_folder(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    let leaf = text
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default();
    crate::guard::is_absolute_without_parent(path)
        && leaf.eq_ignore_ascii_case(Place::ChildPath.folder())
}

/// The places a stand-in sits in, each a folder named for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Beside the program that does the lookup (std's "application path").
    AppDir,
    /// A folder on the `PATH` the lookup's program itself was started with.
    ParentPath,
    /// A folder on a `PATH` the lookup sets for the child alone.
    ChildPath,
    /// The working folder the lookup runs in.
    WorkingDir,
}

impl Place {
    pub const ALL: [Place; 4] = [
        Place::AppDir,
        Place::ParentPath,
        Place::ChildPath,
        Place::WorkingDir,
    ];

    /// The folder's name, which is also what the stand-in in it answers with.
    pub fn folder(self) -> &'static str {
        match self {
            Place::AppDir => "pitboard-probe-lookup-app",
            Place::ParentPath => "pitboard-probe-lookup-parent-path",
            Place::ChildPath => "pitboard-probe-lookup-child-path",
            Place::WorkingDir => "pitboard-probe-lookup-cwd",
        }
    }

    pub fn from_folder(folder: &str) -> Option<Place> {
        Place::ALL
            .into_iter()
            .find(|p| p.folder().eq_ignore_ascii_case(folder))
    }
}

/// One arrangement of stand-ins and one way of looking the name up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    pub label: &'static str,
    /// Where a stand-in `.exe` is put for this case.
    pub exe_in: &'static [Place],
    /// Whether a stand-in `.cmd` (and no `.exe`) is put on the parent's `PATH`.
    pub cmd_on_parent_path: bool,
    /// Whether the lookup sets the child's `PATH` (to the child-path folder, then the rest).
    pub sets_child_path: bool,
}

/// The cases the block runs. The first is F3's question.
pub const CASES: [Case; 6] = [
    Case {
        label: "beside_the_program_and_on_path",
        exe_in: &[Place::AppDir, Place::ParentPath],
        cmd_on_parent_path: false,
        sets_child_path: false,
    },
    Case {
        label: "beside_the_program_and_on_a_changed_child_path",
        exe_in: &[Place::AppDir, Place::ChildPath],
        cmd_on_parent_path: false,
        sets_child_path: true,
    },
    Case {
        label: "on_path_only",
        exe_in: &[Place::ParentPath],
        cmd_on_parent_path: false,
        sets_child_path: false,
    },
    Case {
        label: "in_the_working_folder_only",
        exe_in: &[Place::WorkingDir],
        cmd_on_parent_path: false,
        sets_child_path: false,
    },
    Case {
        label: "a_cmd_on_path_only",
        exe_in: &[],
        cmd_on_parent_path: true,
        sets_child_path: false,
    },
    Case {
        label: "a_cmd_on_path_and_an_exe_on_path",
        exe_in: &[Place::ParentPath],
        cmd_on_parent_path: true,
        sets_child_path: false,
    },
];

/// What the `.cmd` stand-in prints, so a run of it is told from every `.exe`.
pub const CMD_ANSWER: &str = "pitboard-probe-lookup-cmd";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_probe_names_without_an_extension_are_taken() {
        assert!(is_probe_name("pitboard-probe-lookup"));
        for bad in [
            "claude",
            "pitboard-probe-",
            "pitboard-probe-x.exe",
            r"pitboard-probe-..\claude",
            "pitboard-probe-a/b",
            "pitboard-probe-a b",
            "x-pitboard-probe-y",
        ] {
            assert!(!is_probe_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_host_takes_only_the_probes_child_path_folder() {
        assert!(is_child_path_folder(std::path::Path::new(
            r"C:\t\pitboard-probe-lookup\pitboard-probe-lookup-child-path"
        )));
        for bad in [
            r"C:\Windows\System32",
            r"pitboard-probe-lookup-child-path",
            r"C:\t\..\pitboard-probe-lookup-child-path",
            r"C:\t\pitboard-probe-lookup-child-path\x",
        ] {
            assert!(!is_child_path_folder(std::path::Path::new(bad)), "{bad}");
        }
    }

    #[test]
    fn every_place_has_a_probe_folder_that_names_it_back() {
        for p in Place::ALL {
            assert!(p.folder().starts_with(PROBE_PREFIX));
            assert_eq!(Place::from_folder(p.folder()), Some(p));
        }
        assert_eq!(Place::from_folder("elsewhere"), None);
    }

    #[test]
    fn the_first_case_is_f3s_question() {
        let c = CASES[0];
        assert!(c.exe_in.contains(&Place::AppDir) && c.exe_in.contains(&Place::ParentPath));
        assert!(!c.sets_child_path);
        let labels: Vec<_> = CASES.iter().map(|c| c.label).collect();
        let mut unique = labels.clone();
        unique.dedup();
        assert_eq!(labels.len(), unique.len());
    }
}
