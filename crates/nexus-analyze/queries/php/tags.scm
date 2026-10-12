; Rename Forge tags (php)
(class_declaration name: (name) @name) @def.class
(trait_declaration name: (name) @name) @def.class
(interface_declaration name: (name) @name) @def.interface
(enum_declaration name: (name) @name) @def.enum
(enum_case name: (name) @name) @def.enum_member
(function_definition name: (name) @name) @def.function
(method_declaration name: (name) @name) @def.method
(const_element (name) @name) @def.constant
(property_element name: (variable_name (name) @name)) @def.field
(property_promotion_parameter name: (variable_name (name) @name)) @def.field
(simple_parameter name: (variable_name (name) @name)) @def.parameter
(variadic_parameter name: (variable_name (name) @name)) @def.parameter
(assignment_expression left: (variable_name (name) @name)) @def.variable
(foreach_statement (variable_name (name) @name)) @def.variable
(foreach_statement (pair (variable_name (name) @name))) @def.variable

(member_access_expression name: (name) @ref.member)
(nullsafe_member_access_expression name: (name) @ref.member)
(member_call_expression name: (name) @ref.member)
(scoped_call_expression name: (name) @ref.member)
(scoped_property_access_expression name: (variable_name (name) @ref.member))
(class_constant_access_expression (name) (name) @ref.member)

(namespace_use_clause (qualified_name) @import.module)
(namespace_use_clause (name) @import.module)
(namespace_definition name: (namespace_name) @import.module)
(require_expression (string) @import.module)
(require_once_expression (string) @import.module)
(include_expression (string) @import.module)
(include_once_expression (string) @import.module)

(name) @ref
(string) @string
