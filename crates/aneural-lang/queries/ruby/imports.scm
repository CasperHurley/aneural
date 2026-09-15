(call
  method: (identifier) @method
  arguments: (argument_list . (string) @source)
  (#match? @method "^(require|require_relative|load)$")) @call
