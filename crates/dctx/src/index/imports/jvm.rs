// SPDX-License-Identifier: Apache-2.0
//! Match the declared package and qualified declaration independent of its source path.
use super::{RefForm, Tables, language_of};

impl Tables<'_> {
    pub(super) fn jvm_target(&self, importer: &str, name: &str) -> Option<(u32, bool)> {
        let (name, static_member) = name
            .strip_prefix("static ")
            .map_or((name, false), |n| (n, true));
        let simple = name.rsplit('.').next()?;
        let mut hits = self
            .bare
            .get(&("jvm", simple))?
            .iter()
            .copied()
            .filter(|&i| {
                let node = &self.nodes[i as usize];
                let module = self.file_of(i);
                let package = self
                    .facts(module)
                    .iter()
                    .find(|r| r.form == RefForm::Package)
                    .map_or("", |r| r.name.as_str());
                let local = node.name.replace("::", ".");
                let qualified = if package.is_empty() {
                    local
                } else {
                    format!("{package}.{local}")
                };
                if qualified != name {
                    return false;
                }
                let src = self.text.get(node.path);
                let is_java = language_of(src) == "java";
                if is_java {
                    let same_package = self.module_paths.get(importer).is_some_and(|&m| {
                        self.facts(m)
                            .iter()
                            .any(|r| r.form == RefForm::Package && r.name == package)
                    });
                    let public = self
                        .facts(module)
                        .iter()
                        .any(|r| r.form == RefForm::Public && r.name == node.name.as_str());
                    if !(public || same_package)
                        || node.signature.split_whitespace().any(|w| w == "private")
                    {
                        return false;
                    }
                    if static_member {
                        return node.signature.split_whitespace().any(|w| w == "static");
                    }
                    return matches!(
                        node.kind.as_str(),
                        "class" | "enum" | "interface" | "record" | "type"
                    );
                }
                !static_member && !node.signature.split_whitespace().any(|w| w == "private")
            });
        let hit = hits.next()?;
        hits.next().is_none().then_some((hit, false))
    }
}
