use crate::discovery::scanner::{MarkerInfo, TestCase, TestModule};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub name: String,
    pub file_path: PathBuf,
    pub bases: Vec<String>,
    pub methods: Vec<TestCase>,
    pub is_test_class: bool,
    pub class_markers: (Vec<String>, Vec<MarkerInfo>),
    pub line_number: usize,
}

/// Resolve transitive class inheritance and propagate inherited test methods.
///
/// Two-phase fixed-point algorithm:
/// 1. Propagate `is_test_class` from parent to child transitively
/// 2. Copy parent test methods to children (skip overridden methods)
pub fn resolve_inheritance(class_defs: &[ClassInfo]) -> Vec<ClassInfo> {
    let mut resolved: Vec<ClassInfo> = class_defs.to_vec();

    let mut name_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, class) in resolved.iter().enumerate() {
        name_to_indices
            .entry(class.name.clone())
            .or_default()
            .push(i);
    }

    // Phase 1: Propagate is_test_class transitively
    loop {
        let mut changed = false;
        for i in 0..resolved.len() {
            if resolved[i].is_test_class {
                continue;
            }
            let bases = resolved[i].bases.clone();
            let child_file = resolved[i].file_path.clone();
            for base_name in &bases {
                let simple_name = base_name.rsplit('.').next().unwrap_or(base_name);
                if let Some(indices) = name_to_indices.get(simple_name) {
                    let is_parent_test_class = indices
                        .iter()
                        .find(|&&idx| {
                            resolved[idx].file_path == child_file && resolved[idx].is_test_class
                        })
                        .or_else(|| indices.iter().find(|&&idx| resolved[idx].is_test_class))
                        .is_some();
                    if is_parent_test_class {
                        resolved[i].is_test_class = true;
                        changed = true;
                        break;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Phase 2: Propagate inherited methods
    loop {
        let mut changed = false;
        for i in 0..resolved.len() {
            if !resolved[i].is_test_class {
                continue;
            }
            let child_class_name = resolved[i].name.clone();
            let child_file = resolved[i].file_path.clone();

            let existing_method_names: HashSet<String> = resolved[i]
                .methods
                .iter()
                .map(|m| m.name.rsplit("::").next().unwrap_or(&m.name).to_string())
                .collect();

            let mut inherited_methods: Vec<TestCase> = vec![];
            let bases = resolved[i].bases.clone();
            for base_name in &bases {
                let simple_name = base_name.rsplit('.').next().unwrap_or(base_name);
                if let Some(indices) = name_to_indices.get(simple_name) {
                    let parent_idx = indices
                        .iter()
                        .find(|&&idx| resolved[idx].file_path == child_file)
                        .or_else(|| indices.first())
                        .copied();
                    if let Some(pidx) = parent_idx {
                        for parent_method in &resolved[pidx].methods.clone() {
                            let method_simple = parent_method
                                .name
                                .rsplit("::")
                                .next()
                                .unwrap_or(&parent_method.name);
                            if !existing_method_names.contains(method_simple)
                                && !inherited_methods.iter().any(|m| {
                                    m.name.rsplit("::").next().unwrap_or(&m.name) == method_simple
                                })
                            {
                                let mut inherited = parent_method.clone();
                                inherited.name = format!("{}::{}", child_class_name, method_simple);
                                inherited_methods.push(inherited);
                            }
                        }
                    }
                }
            }
            if !inherited_methods.is_empty() {
                resolved[i].methods.extend(inherited_methods);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    resolved
}

/// Apply resolved inheritance back to TestModules.
///
/// Injects newly-discovered tests from inheritance resolution into the
/// appropriate modules.
pub fn apply_resolved_classes(modules: &mut [TestModule], resolved: &[ClassInfo]) {
    let resolved_map: HashMap<(PathBuf, String), &ClassInfo> = resolved
        .iter()
        .map(|c| ((c.file_path.clone(), c.name.clone()), c))
        .collect();

    for module in modules.iter_mut() {
        for class_def in &module.class_defs {
            if let Some(resolved_class) =
                resolved_map.get(&(module.path.clone(), class_def.name.clone()))
            {
                if !resolved_class.is_test_class {
                    continue;
                }

                let existing_prefix = format!("{}::", class_def.name);
                let existing_methods: HashSet<String> = module
                    .tests
                    .iter()
                    .filter(|t| t.name.starts_with(&existing_prefix) || t.name == class_def.name)
                    .map(|t| t.name.clone())
                    .collect();

                for method in &resolved_class.methods {
                    if !existing_methods.contains(&method.name) {
                        module.tests.push(method.clone());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_case(name: &str) -> TestCase {
        TestCase {
            name: name.to_string(),
            dependencies: vec![],
            is_async: false,
            line_number: 1,
            parametrized_args: vec![],
            timeout_secs: None,
            markers: vec![],
            marker_info: vec![],
            param_id: None,
        }
    }

    #[test]
    fn test_class_info_creation() {
        let info = ClassInfo {
            name: "TestFoo".to_string(),
            file_path: PathBuf::from("test_foo.py"),
            bases: vec!["unittest.TestCase".to_string()],
            methods: vec![],
            is_test_class: true,
            class_markers: (vec![], vec![]),
            line_number: 1,
        };
        assert_eq!(info.name, "TestFoo");
        assert!(info.is_test_class);
    }

    #[test]
    fn test_transitive_test_class_detection() {
        let class_defs = vec![
            ClassInfo {
                name: "BaseLoader".into(),
                file_path: "base.py".into(),
                bases: vec!["unittest.TestCase".into()],
                methods: vec![make_test_case("BaseLoader::test_load")],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "CustomLoader".into(),
                file_path: "custom.py".into(),
                bases: vec!["BaseLoader".into()],
                methods: vec![make_test_case("CustomLoader::test_custom")],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let custom = resolved.iter().find(|c| c.name == "CustomLoader").unwrap();
        assert!(
            custom.is_test_class,
            "CustomLoader should be transitively detected as test class"
        );
    }

    #[test]
    fn test_three_level_transitive_inheritance() {
        let class_defs = vec![
            ClassInfo {
                name: "GrandParent".into(),
                file_path: "base.py".into(),
                bases: vec!["TestCase".into()],
                methods: vec![make_test_case("GrandParent::test_gp")],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "Parent".into(),
                file_path: "mid.py".into(),
                bases: vec!["GrandParent".into()],
                methods: vec![],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "Child".into(),
                file_path: "child.py".into(),
                bases: vec!["Parent".into()],
                methods: vec![make_test_case("Child::test_child")],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let parent = resolved.iter().find(|c| c.name == "Parent").unwrap();
        assert!(parent.is_test_class);
        let child = resolved.iter().find(|c| c.name == "Child").unwrap();
        assert!(child.is_test_class);
    }

    #[test]
    fn test_inherited_methods_propagated() {
        let class_defs = vec![
            ClassInfo {
                name: "ReloaderTests".into(),
                file_path: "base.py".into(),
                bases: vec![],
                methods: vec![
                    make_test_case("ReloaderTests::test_glob"),
                    make_test_case("ReloaderTests::test_glob_recursive"),
                ],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "StatReloaderTests".into(),
                file_path: "stat.py".into(),
                bases: vec!["ReloaderTests".into()],
                methods: vec![],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 5,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let stat = resolved
            .iter()
            .find(|c| c.name == "StatReloaderTests")
            .unwrap();
        assert_eq!(stat.methods.len(), 2);
        assert!(
            stat.methods
                .iter()
                .any(|m| m.name == "StatReloaderTests::test_glob")
        );
        assert!(
            stat.methods
                .iter()
                .any(|m| m.name == "StatReloaderTests::test_glob_recursive")
        );
    }

    #[test]
    fn test_inherited_methods_not_duplicated_when_overridden() {
        let class_defs = vec![
            ClassInfo {
                name: "ParentTests".into(),
                file_path: "parent.py".into(),
                bases: vec![],
                methods: vec![
                    make_test_case("ParentTests::test_foo"),
                    make_test_case("ParentTests::test_bar"),
                ],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "ChildTests".into(),
                file_path: "child.py".into(),
                bases: vec!["ParentTests".into()],
                methods: vec![make_test_case("ChildTests::test_foo")],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let child = resolved.iter().find(|c| c.name == "ChildTests").unwrap();
        assert_eq!(child.methods.len(), 2);
        assert!(
            child
                .methods
                .iter()
                .any(|m| m.name == "ChildTests::test_foo")
        );
        assert!(
            child
                .methods
                .iter()
                .any(|m| m.name == "ChildTests::test_bar")
        );
    }

    #[test]
    fn test_inherited_methods_multi_level() {
        let class_defs = vec![
            ClassInfo {
                name: "A".into(),
                file_path: "a.py".into(),
                bases: vec!["TestCase".into()],
                methods: vec![make_test_case("A::test_a")],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "B".into(),
                file_path: "b.py".into(),
                bases: vec!["A".into()],
                methods: vec![make_test_case("B::test_b")],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "C".into(),
                file_path: "c.py".into(),
                bases: vec!["B".into()],
                methods: vec![],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let c = resolved.iter().find(|c| c.name == "C").unwrap();
        assert!(c.is_test_class);
        assert_eq!(c.methods.len(), 2);
        assert!(c.methods.iter().any(|m| m.name == "C::test_a"));
        assert!(c.methods.iter().any(|m| m.name == "C::test_b"));
    }

    #[test]
    fn test_non_test_class_not_promoted_without_test_ancestor() {
        let class_defs = vec![
            ClassInfo {
                name: "Helper".into(),
                file_path: "helper.py".into(),
                bases: vec![],
                methods: vec![make_test_case("Helper::test_thing")],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "DerivedHelper".into(),
                file_path: "derived.py".into(),
                bases: vec!["Helper".into()],
                methods: vec![],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let derived = resolved.iter().find(|c| c.name == "DerivedHelper").unwrap();
        assert!(
            !derived.is_test_class,
            "Should NOT be promoted without a test ancestor"
        );
    }

    #[test]
    fn test_same_file_class_preferred_for_inheritance() {
        let class_defs = vec![
            ClassInfo {
                name: "Base".into(),
                file_path: "a.py".into(),
                bases: vec!["TestCase".into()],
                methods: vec![make_test_case("Base::test_a")],
                is_test_class: true,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "Base".into(),
                file_path: "b.py".into(),
                bases: vec![],
                methods: vec![make_test_case("Base::test_b")],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 1,
            },
            ClassInfo {
                name: "Child".into(),
                file_path: "a.py".into(),
                bases: vec!["Base".into()],
                methods: vec![],
                is_test_class: false,
                class_markers: (vec![], vec![]),
                line_number: 10,
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let child = resolved.iter().find(|c| c.name == "Child").unwrap();
        assert!(child.is_test_class);
        assert!(child.methods.iter().any(|m| m.name == "Child::test_a"));
    }
}
