local m = require("@lam")
return m:update("a", function(v) return v + 1 end, 0)
