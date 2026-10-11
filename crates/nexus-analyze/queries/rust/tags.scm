; Rename Forge tags (rust)
(function_item name: (identifier) @name) @def.function
(function_signature_item name: (identifier) @name) @def.function
(struct_item name: (type_identifier) @name) @def.class
(union_item name: (type_identifier) @name) @def.class
(type_item name: (type_identifier) @name) @def.class
(enum_item name: (type_identifier) @name) @def.enum
(enum_variant name: (identifier) @name) @def.enum_member
(trait_item name: (type_identifier) @name) @def.interface
(const_item name: (identifier) @name) @def.constant
(static_item name: (identifier) @name) @def.constant
(mod_item name: (identifier) @name) @def.module
(field_declaration name: (field_identifier) @name) @def.field
(let_declaration pattern: (identifier) @name) @def.variable
(parameter pattern: (identifier) @name) @def.parameter
(closure_parameters (identifier) @name @def.parameter)

(field_expression field: (field_identifier) @ref.member)

(mod_item name: (identifier) @import.module !body)
(use_declaration argument: (_) @import.module)

[(identifier) (type_identifier) (field_identifier)] @ref
(string_literal) @string
