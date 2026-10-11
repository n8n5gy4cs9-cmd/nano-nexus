; Rename Forge tags (java)
(class_declaration name: (identifier) @name) @def.class
(record_declaration name: (identifier) @name) @def.class
(interface_declaration name: (identifier) @name) @def.interface
(enum_declaration name: (identifier) @name) @def.enum
(enum_constant name: (identifier) @name) @def.enum_member
(method_declaration name: (identifier) @name) @def.method
(field_declaration declarator: (variable_declarator name: (identifier) @name)) @def.field
(local_variable_declaration declarator: (variable_declarator name: (identifier) @name)) @def.variable
(formal_parameter name: (identifier) @name) @def.parameter
(catch_formal_parameter name: (identifier) @name) @def.parameter

(field_access field: (identifier) @ref.member)
(method_invocation object: (_) name: (identifier) @ref.member)

(import_declaration [(scoped_identifier) (identifier)] @import.module)

[(identifier) (type_identifier)] @ref
(string_literal) @string
