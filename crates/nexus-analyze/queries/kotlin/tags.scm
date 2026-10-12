; Rename Forge tags (kotlin)
(class_declaration name: (identifier) @name) @def.class
(object_declaration name: (identifier) @name) @def.class
(function_declaration name: (identifier) @name) @def.function
(type_alias (identifier) @name) @def.class
(enum_entry (identifier) @name) @def.enum_member
(property_declaration (variable_declaration (identifier) @name)) @def.variable
(class_parameter (identifier) @name) @def.field
(parameter (identifier) @name) @def.parameter

(navigation_expression (identifier) @ref.member .)

(import (qualified_identifier) @import.module)

(identifier) @ref
(string_literal) @string
