; Rename Forge tags (c#)
(class_declaration name: (identifier) @name) @def.class
(struct_declaration name: (identifier) @name) @def.class
(record_declaration name: (identifier) @name) @def.class
(interface_declaration name: (identifier) @name) @def.interface
(enum_declaration name: (identifier) @name) @def.enum
(enum_member_declaration name: (identifier) @name) @def.enum_member
(method_declaration name: (identifier) @name) @def.method
(property_declaration name: (identifier) @name) @def.field
(variable_declarator name: (identifier) @name) @def.variable
(parameter name: (identifier) @name) @def.parameter

(member_access_expression name: (identifier) @ref.member)

(using_directive [(identifier) (qualified_name)] @import.module)

(identifier) @ref
(string_literal) @string
