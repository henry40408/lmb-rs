local m = require("@lam")
local a = m.get("a") or 1
a = a + 1
m.set("a", a)
return a
