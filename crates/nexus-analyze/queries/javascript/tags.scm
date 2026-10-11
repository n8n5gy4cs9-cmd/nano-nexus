; Rename Forge tags (javascript, jsx)
(function_declaration name: (identifier) @name) @def.function
(generator_function_declaration name: (identifier) @name) @def.function
(class_declaration name: (identifier) @name) @def.class
(method_definition name: (property_identifier) @name) @def.method
(field_definition property: (property_identifier) @name) @def.field
(variable_declarator name: (identifier) @name) @def.variable
(formal_parameters (identifier) @name @def.parameter)
(formal_parameters (assignment_pattern left: (identifier) @name @def.parameter))
(arrow_function parameter: (identifier) @name @def.parameter)

(member_expression property: (property_identifier) @ref.member)

(import_statement source: (string) @import.module)
(export_statement source: (string) @import.module)
(import_specifier name: (identifier) @import.name)
(call_expression
  function: (identifier) @_fn
  arguments: (arguments . (string) @import.module)
  (#eq? @_fn "require"))

[(identifier) (property_identifier) (shorthand_property_identifier) (shorthand_property_identifier_pattern)] @ref
(string) @string
