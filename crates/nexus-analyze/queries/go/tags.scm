; Rename Forge tags (go)
(function_declaration name: (identifier) @name) @def.function
(method_declaration name: (field_identifier) @name) @def.method
(type_spec name: (type_identifier) @name) @def.class
(const_spec name: (identifier) @name) @def.constant
(var_spec name: (identifier) @name) @def.variable
(short_var_declaration left: (expression_list (identifier) @name @def.variable))
(range_clause left: (expression_list (identifier) @name @def.variable))
(parameter_declaration name: (identifier) @name) @def.parameter
(variadic_parameter_declaration name: (identifier) @name) @def.parameter
(field_declaration name: (field_identifier) @name) @def.field
(method_elem name: (field_identifier) @name) @def.method

(selector_expression field: (field_identifier) @ref.member)

(import_spec path: (interpreted_string_literal) @import.module)

[(identifier) (type_identifier) (field_identifier) (package_identifier)] @ref
(interpreted_string_literal) @string
