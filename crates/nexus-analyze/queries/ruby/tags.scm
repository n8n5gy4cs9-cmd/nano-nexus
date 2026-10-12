; Rename Forge tags (ruby)
(class name: (constant) @name) @def.class
(module name: (constant) @name) @def.class
(method name: (identifier) @name) @def.method
(singleton_method name: (identifier) @name) @def.method
(assignment left: (constant) @name) @def.constant
(assignment left: (identifier) @name) @def.variable
(operator_assignment left: (identifier) @name) @def.variable
(method_parameters (identifier) @name @def.parameter)
(optional_parameter name: (identifier) @name) @def.parameter
(keyword_parameter name: (identifier) @name) @def.parameter
(splat_parameter name: (identifier) @name) @def.parameter
(block_parameter name: (identifier) @name) @def.parameter
(block_parameters (identifier) @name @def.parameter)

(call receiver: (_) method: (identifier) @ref.member)

(call
  method: (identifier) @_f
  arguments: (argument_list . (string) @import.module)
  (#match? @_f "^require(_relative)?$"))

[(identifier) (constant)] @ref
(string) @string
