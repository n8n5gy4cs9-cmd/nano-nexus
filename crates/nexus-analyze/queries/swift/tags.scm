; Rename Forge tags (swift)
(class_declaration name: (type_identifier) @name) @def.class
(protocol_declaration name: (type_identifier) @name) @def.interface
(typealias_declaration name: (type_identifier) @name) @def.class
(function_declaration name: (simple_identifier) @name) @def.function
(protocol_function_declaration name: (simple_identifier) @name) @def.method
(enum_entry name: (simple_identifier) @name) @def.enum_member
(property_declaration name: (pattern bound_identifier: (simple_identifier) @name)) @def.variable
(protocol_property_declaration name: (pattern bound_identifier: (simple_identifier) @name)) @def.field
(parameter name: (simple_identifier) @name) @def.parameter

(navigation_suffix suffix: (simple_identifier) @ref.member)

(import_declaration (identifier) @import.module)

[(simple_identifier) (type_identifier)] @ref
(line_string_literal) @string
