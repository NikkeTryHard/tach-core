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
    pub has_testcase_base: bool,
    pub has_test_false: bool,
    pub has_init: bool,
    pub has_new: bool,
    pub is_abstract: bool,
}

/// Resolve transitive class inheritance and propagate inherited test methods.
///
/// Two-phase fixed-point algorithm:
/// 1. Propagate `is_test_class` and `has_testcase_base` from parent to child transitively
/// 2. Copy parent test methods to children (skip overridden methods)
///
/// Respects pytest exclusion rules:
/// - `__test__ = False` prevents collection
/// - `__init__`/`__new__` prevents collection for non-unittest classes
/// - Abstract classes (with unresolved `@abstractmethod`) are skipped
pub fn resolve_inheritance(class_defs: &[ClassInfo]) -> Vec<ClassInfo> {
    let mut resolved: Vec<ClassInfo> = class_defs.to_vec();

    let mut name_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, class) in resolved.iter().enumerate() {
        name_to_indices
            .entry(class.name.clone())
            .or_default()
            .push(i);
    }

    // Phase 1: Propagate is_test_class and has_testcase_base transitively
    loop {
        let mut changed = false;
        for i in 0..resolved.len() {
            let bases = resolved[i].bases.clone();
            let child_file = resolved[i].file_path.clone();
            for base_name in &bases {
                let simple_name = base_name.rsplit('.').next().unwrap_or(base_name);
                if let Some(indices) = name_to_indices.get(simple_name) {
                    let parent_idx = indices
                        .iter()
                        .find(|&&idx| resolved[idx].file_path == child_file)
                        .or_else(|| indices.first())
                        .copied();

                    if let Some(pidx) = parent_idx {
                        if resolved[pidx].has_testcase_base && !resolved[i].has_testcase_base {
                            resolved[i].has_testcase_base = true;
                            changed = true;
                        }
                        if !resolved[i].is_test_class && resolved[pidx].is_test_class {
                            resolved[i].is_test_class = true;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Phase 1.5: Apply exclusion rules after inheritance propagation
    for class in resolved.iter_mut() {
        if class.has_test_false {
            class.is_test_class = false;
            continue;
        }
        if class.is_abstract {
            class.is_test_class = false;
            continue;
        }
        // __init__/__new__ only excludes non-unittest classes (pytest behavior)
        if (class.has_init || class.has_new) && !class.has_testcase_base {
            class.is_test_class = false;
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
        let mut tests_to_remove: HashSet<String> = HashSet::new();

        for class_def in &module.class_defs {
            if let Some(resolved_class) =
                resolved_map.get(&(module.path.clone(), class_def.name.clone()))
            {
                let class_prefix = format!("{}::", class_def.name);

                if !resolved_class.is_test_class {
                    for test in &module.tests {
                        if test.name.starts_with(&class_prefix) {
                            tests_to_remove.insert(test.name.clone());
                        }
                    }
                    continue;
                }

                let existing_methods: HashSet<String> = module
                    .tests
                    .iter()
                    .filter(|t| t.name.starts_with(&class_prefix) || t.name == class_def.name)
                    .map(|t| t.name.clone())
                    .collect();

                for method in &resolved_class.methods {
                    if !existing_methods.contains(&method.name) {
                        module.tests.push(method.clone());
                    }
                }
            }
        }

        if !tests_to_remove.is_empty() {
            module.tests.retain(|t| !tests_to_remove.contains(&t.name));
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

    fn make_class(
        name: &str,
        file: &str,
        bases: Vec<String>,
        methods: Vec<TestCase>,
        is_test_class: bool,
    ) -> ClassInfo {
        ClassInfo {
            name: name.into(),
            file_path: file.into(),
            bases,
            methods,
            is_test_class,
            class_markers: (vec![], vec![]),
            line_number: 1,
            has_testcase_base: false,
            has_test_false: false,
            has_init: false,
            has_new: false,
            is_abstract: false,
        }
    }

    #[test]
    fn test_class_info_creation() {
        let info = make_class(
            "TestFoo",
            "test_foo.py",
            vec!["unittest.TestCase".into()],
            vec![],
            true,
        );
        assert_eq!(info.name, "TestFoo");
        assert!(info.is_test_class);
    }

    #[test]
    fn test_transitive_test_class_detection() {
        let class_defs = vec![
            make_class(
                "BaseLoader",
                "base.py",
                vec!["unittest.TestCase".into()],
                vec![make_test_case("BaseLoader::test_load")],
                true,
            ),
            make_class(
                "CustomLoader",
                "custom.py",
                vec!["BaseLoader".into()],
                vec![make_test_case("CustomLoader::test_custom")],
                false,
            ),
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
            make_class(
                "GrandParent",
                "base.py",
                vec!["TestCase".into()],
                vec![make_test_case("GrandParent::test_gp")],
                true,
            ),
            make_class(
                "Parent",
                "mid.py",
                vec!["GrandParent".into()],
                vec![],
                false,
            ),
            make_class(
                "Child",
                "child.py",
                vec!["Parent".into()],
                vec![make_test_case("Child::test_child")],
                false,
            ),
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
            make_class(
                "ReloaderTests",
                "base.py",
                vec![],
                vec![
                    make_test_case("ReloaderTests::test_glob"),
                    make_test_case("ReloaderTests::test_glob_recursive"),
                ],
                true,
            ),
            make_class(
                "StatReloaderTests",
                "stat.py",
                vec!["ReloaderTests".into()],
                vec![],
                true,
            ),
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
            make_class(
                "ParentTests",
                "parent.py",
                vec![],
                vec![
                    make_test_case("ParentTests::test_foo"),
                    make_test_case("ParentTests::test_bar"),
                ],
                true,
            ),
            make_class(
                "ChildTests",
                "child.py",
                vec!["ParentTests".into()],
                vec![make_test_case("ChildTests::test_foo")],
                true,
            ),
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
            make_class(
                "A",
                "a.py",
                vec!["TestCase".into()],
                vec![make_test_case("A::test_a")],
                true,
            ),
            make_class(
                "B",
                "b.py",
                vec!["A".into()],
                vec![make_test_case("B::test_b")],
                false,
            ),
            make_class("C", "c.py", vec!["B".into()], vec![], false),
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
            make_class(
                "Helper",
                "helper.py",
                vec![],
                vec![make_test_case("Helper::test_thing")],
                false,
            ),
            make_class(
                "DerivedHelper",
                "derived.py",
                vec!["Helper".into()],
                vec![],
                false,
            ),
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
            make_class(
                "Base",
                "a.py",
                vec!["TestCase".into()],
                vec![make_test_case("Base::test_a")],
                true,
            ),
            make_class(
                "Base",
                "b.py",
                vec![],
                vec![make_test_case("Base::test_b")],
                false,
            ),
            make_class("Child", "a.py", vec!["Base".into()], vec![], false),
        ];

        let resolved = resolve_inheritance(&class_defs);
        let child = resolved.iter().find(|c| c.name == "Child").unwrap();
        assert!(child.is_test_class);
        assert!(child.methods.iter().any(|m| m.name == "Child::test_a"));
    }

    #[test]
    fn test_test_false_prevents_collection() {
        let mut class = make_class(
            "TestFoo",
            "test_foo.py",
            vec!["TestCase".into()],
            vec![make_test_case("TestFoo::test_bar")],
            true,
        );
        class.has_test_false = true;
        class.has_testcase_base = true;

        let resolved = resolve_inheritance(&[class]);
        assert!(!resolved[0].is_test_class);
    }

    #[test]
    fn test_init_excludes_non_unittest_class() {
        let mut class = make_class(
            "TestWidget",
            "test_widget.py",
            vec![],
            vec![make_test_case("TestWidget::test_render")],
            true,
        );
        class.has_init = true;

        let resolved = resolve_inheritance(&[class]);
        assert!(!resolved[0].is_test_class);
    }

    #[test]
    fn test_init_does_not_exclude_unittest_class() {
        let mut class = make_class(
            "TestWidget",
            "test_widget.py",
            vec!["TestCase".into()],
            vec![make_test_case("TestWidget::test_render")],
            true,
        );
        class.has_init = true;
        class.has_testcase_base = true;

        let resolved = resolve_inheritance(&[class]);
        assert!(resolved[0].is_test_class);
    }

    #[test]
    fn test_abstract_class_excluded() {
        let mut class = make_class(
            "TestBase",
            "test_base.py",
            vec!["TestCase".into()],
            vec![make_test_case("TestBase::test_stuff")],
            true,
        );
        class.is_abstract = true;
        class.has_testcase_base = true;

        let resolved = resolve_inheritance(&[class]);
        assert!(!resolved[0].is_test_class);
    }

    #[test]
    fn test_new_excludes_non_unittest_class() {
        let mut class = make_class(
            "TestSingleton",
            "test_s.py",
            vec![],
            vec![make_test_case("TestSingleton::test_instance")],
            true,
        );
        class.has_new = true;

        let resolved = resolve_inheritance(&[class]);
        assert!(!resolved[0].is_test_class);
    }

    #[test]
    fn test_mixin_class_not_collected_without_testcase_base() {
        let class_defs = vec![
            make_class(
                "IntegrationTests",
                "test_auto.py",
                vec![],
                vec![
                    make_test_case("IntegrationTests::test_glob"),
                    make_test_case("IntegrationTests::test_multiple"),
                ],
                false,
            ),
            {
                let mut child = make_class(
                    "WatchmanReloaderTests",
                    "test_auto.py",
                    vec!["ReloaderTests".into(), "IntegrationTests".into()],
                    vec![],
                    true,
                );
                child.has_testcase_base = true;
                child
            },
        ];

        let resolved = resolve_inheritance(&class_defs);
        let mixin = resolved
            .iter()
            .find(|c| c.name == "IntegrationTests")
            .unwrap();
        assert!(
            !mixin.is_test_class,
            "Mixin without TestCase base should not be collected"
        );
        let child = resolved
            .iter()
            .find(|c| c.name == "WatchmanReloaderTests")
            .unwrap();
        assert!(child.is_test_class);
    }

    #[test]
    fn test_testcase_base_propagated_transitively() {
        let class_defs = vec![
            {
                let mut c = make_class(
                    "BaseTest",
                    "base.py",
                    vec!["TestCase".into()],
                    vec![make_test_case("BaseTest::test_base")],
                    true,
                );
                c.has_testcase_base = true;
                c
            },
            make_class("MidLevel", "mid.py", vec!["BaseTest".into()], vec![], false),
            make_class(
                "LeafTest",
                "leaf.py",
                vec!["MidLevel".into()],
                vec![],
                false,
            ),
        ];

        let resolved = resolve_inheritance(&class_defs);
        let leaf = resolved.iter().find(|c| c.name == "LeafTest").unwrap();
        assert!(leaf.is_test_class);
        assert!(leaf.has_testcase_base);
    }
}
