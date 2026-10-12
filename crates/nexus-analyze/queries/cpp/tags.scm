; Rename Forge tags (c++)
(function_definition declarator: (function_declarator declarator: (identifier) @name)) @def.function
(function_definition declarator: (function_declarator declarator: (field_identifier) @name)) @def.method
(function_definition declarator: (function_declarator declarator: (qualified_identifier name: (identifier) @name))) @def.method
(function_definition declarator: (pointer_declarator declarator: (function_declarator declarator: (identifier) @name))) @def.function
(function_definition declarator: (reference_declarator (function_declarator declarator: (identifier) @name))) @def.function
(class_specifier name: (type_identifier) @name body: (_)) @def.class
(struct_specifier name: (type_identifier) @name body: (_)) @def.class
(enum_specifier name: (type_identifier) @name body: (_)) @def.enum
(alias_declaration name: (type_identifier) @name) @def.class
(type_definition declarator: (type_identifier) @name) @def.class
(namespace_definition name: (namespace_identifier) @name) @def.module
(enumerator name: (identifier) @name) @def.enum_member
(preproc_def name: (identifier) @name) @def.constant
(preproc_function_def name: (identifier) @name) @def.function
(field_declaration declarator: (field_identifier) @name) @def.field
(field_declaration declarator: (pointer_declarator declarator: (field_identifier) @name)) @def.field
(init_declarator declarator: (identifier) @name) @def.variable
(init_declarator declarator: (pointer_declarator declarator: (identifier) @name)) @def.variable
(declaration declarator: (identifier) @name) @def.variable
(parameter_declaration declarator: (identifier) @name) @def.parameter
(parameter_declaration declarator: (pointer_declarator declarator: (identifier) @name)) @def.parameter
(parameter_declaration declarator: (reference_declarator (identifier) @name)) @def.parameter

(field_expression field: (field_identifier) @ref.member)

(preproc_include path: (string_literal) @import.module)

[(identifier) (type_identifier) (field_identifier) (namespace_identifier)] @ref
(string_literal) @string
