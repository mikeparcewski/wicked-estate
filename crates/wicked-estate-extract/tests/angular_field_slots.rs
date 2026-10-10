//! TS-S3 base plane: the properties and accessors of decorated classes (`@Component`, `@ng.Component`, `@Cmp`) get their
//! class-owned `:field:` value slot (the endpoint compiler-resolved bindings project onto), and
//! `abstract class` declarations are `Class` nodes. Ordinary classes gain no value slots.

use wicked_estate_core::{Extractor, Language, NodeKind, SourceFile, field_slot_id};
use wicked_estate_extract::treesitter::TreeSitterExtractor;

fn extract(lang: &str, code: &str) -> wicked_estate_core::Extraction {
    TreeSitterExtractor::for_language(lang)
        .unwrap()
        .extract(&SourceFile {
            path: format!("src/a.{}", if lang == "tsx" { "tsx" } else { "ts" }),
            language: Language::new(lang),
            text: code.to_string(),
        })
        .unwrap()
}

const SRC: &str = r#"
import { Component, Directive, Input, input } from '@angular/core';

@Directive()
export abstract class Base {
  @Input() inherited = '';
}

@Component({ selector: 'app-c', template: '' })
export class Cmp extends Base {
  current = 'a';
  name = input<string>('');
}

export class Plain {
  ignored = 1;
}

import * as ng from '@angular/core';
import { Component as Cmp } from '@angular/core';

@ng.Component({ selector: 'app-ns', template: '' })
export class Ns {
  nsField = 'n';
  get computed(): string { return 'g'; }
}

@Cmp({ selector: 'app-alias', template: '' })
class Alias {
  @Input() set badge(v: string) {}
}
"#;

#[test]
fn angular_class_properties_get_field_slots_and_abstract_classes_are_classes() {
    for lang in ["typescript", "tsx"] {
        let ex = extract(lang, SRC);
        let class = |name: &str| {
            ex.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Class && n.name == name)
                .unwrap_or_else(|| panic!("{lang}: class {name} in {:?}", ex.nodes))
                .symbol
                .clone()
        };
        let slots: Vec<_> = ex
            .nodes
            .iter()
            .filter(|n| n.is_value_flow_node() && n.kind == NodeKind::Field)
            .map(|n| n.symbol.clone())
            .collect();
        for (owner, field) in [
            ("Cmp", "current"),
            ("Cmp", "name"),
            ("Base", "inherited"),
            ("Ns", "nsField"),  // namespace decorator
            ("Ns", "computed"), // getter
            ("Alias", "badge"), // renamed decorator, setter input
        ] {
            let want = field_slot_id(&class(owner), field);
            assert!(
                slots.contains(&want),
                "{lang}: {owner}.{field} slot missing: {slots:?}"
            );
        }
        let plain = field_slot_id(&class("Plain"), "ignored");
        assert!(
            !slots.contains(&plain),
            "{lang}: an undecorated class gains no slot"
        );
    }
}
