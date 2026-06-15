; Dart (arborium-dart) — @code_* convention.
; Verified node types: class_definition.name=identifier; function_signature.name=identifier;
; enum_declaration.name=identifier; library_import (import_specification uri).
(class_definition
  name: (identifier) @code_class.name) @code_class.def

(function_signature
  name: (identifier) @code_function.name) @code_function.def

(enum_declaration
  name: (identifier) @code_type.name) @code_type.def

; Import statements — import 'dart:async'; import 'package:foo/bar.dart';
; import_specification may contain uri directly or wrapped in configurable_uri.
(library_import
  (import_specification
    [(uri) (configurable_uri)] @import.source
  )
) @import
