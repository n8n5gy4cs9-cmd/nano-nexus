; Rename Forge tags (typescript, tsx)
(function_declaration name: (identifier) @name) @def.function
(generator_function_declaration name: (identifier) @name) @def.function
(class_declaration name: (type_identifier) @name) @def.class
(abstract_class_declaration name: (type_identifier) @name) @def.class
(interface_declaration name: (type_identifier) @name) @def.interface
(type_alias_declaration name: (type_identifier) @name) @def.class
(enum_declaration name: (identifier) @name) @def.enum
(enum_body name: (property_identifier) @name @def.enum_member)
(enum_assignment name: (property_identifier) @name @def.enum_member)
(method_definition name: (property_identifier) @name) @def.method
(method_signature name: (property_identifier) @name) @def.method
(public_field_definition name: (property_identifier) @name) @def.field
(property_signature name: (property_identifier) @name) @def.field
(variable_declarator name: (identifier) @name) @def.variable
(required_parameter pattern: (identifier) @name) @def.parameter
(optional_parameter pattern: (identifier) @name) @def.parameter
(arrow_function parameter: (identifier) @name @def.parameter)

(member_expression property: (property_identifier) @ref.member)

(import_statement source: (string) @import.module)
(export_statement source: (string) @import.module)
(import_specifier name: (identifier) @import.name)
(call_expression
  function: (identifier) @_fn
  arguments: (arguments . (string) @import.module)
  (#eq? @_fn "require"))

[(identifier) (property_identifier) (type_identifier) (shorthand_property_identifier) (shorthand_property_identifier_pattern)] @ref
(string) @string
