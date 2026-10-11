; Rename Forge tags (python). Captures: @def.<kind> + @name, @ref, @ref.member, @import.module, @import.name, @string
(function_definition name: (identifier) @name) @def.function
(class_definition name: (identifier) @name) @def.class
(assignment left: (identifier) @name) @def.variable
(assignment
  left: (attribute object: (identifier) @_self attribute: (identifier) @name)
  (#eq? @_self "self")) @def.field
(augmented_assignment left: (identifier) @name) @def.variable
(for_statement left: (identifier) @name) @def.variable
(parameters (identifier) @name @def.parameter)
(default_parameter name: (identifier) @name @def.parameter)
(typed_parameter (identifier) @name @def.parameter)
(typed_default_parameter name: (identifier) @name @def.parameter)

(attribute attribute: (identifier) @ref.member)

(import_statement name: (dotted_name) @import.module)
(import_statement name: (aliased_import name: (dotted_name) @import.module))
(import_from_statement module_name: (dotted_name) @import.module)
(import_from_statement module_name: (relative_import) @import.module)
(import_from_statement name: (dotted_name) @import.name)
(import_from_statement name: (aliased_import name: (dotted_name) @import.name))

(identifier) @ref
(string) @string
