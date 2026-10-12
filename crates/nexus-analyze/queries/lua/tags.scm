; Rename Forge tags (lua)
(function_declaration name: (identifier) @name) @def.function
(function_declaration name: (dot_index_expression field: (identifier) @name)) @def.method
(function_declaration name: (method_index_expression method: (identifier) @name)) @def.method
(variable_list name: (identifier) @name @def.variable)
(variable_list name: (dot_index_expression field: (identifier) @name @def.field))
(parameters name: (identifier) @name @def.parameter)
(for_generic_clause (variable_list name: (identifier) @name @def.variable))
(for_numeric_clause name: (identifier) @name) @def.variable
(field name: (identifier) @name) @def.field

(dot_index_expression field: (identifier) @ref.member)
(method_index_expression method: (identifier) @ref.member)

(function_call
  name: (identifier) @_f
  arguments: (arguments . (string content: (string_content) @import.module))
  (#eq? @_f "require"))

(identifier) @ref
(string) @string
