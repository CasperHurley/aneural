; ES module imports (static, type-only, `import x = require()`)
(import_statement) @import

; `export ... from` re-exports
(export_statement source: (string)) @reexport

; dynamic import('x')
(call_expression
  function: (import)
  arguments: (arguments . (string) @dynamic_source)) @dynamic

; CommonJS require('x')
(call_expression
  function: (identifier) @require_fn
  arguments: (arguments . (string) @require_source)
  (#eq? @require_fn "require")) @require
