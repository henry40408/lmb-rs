--[[
--description = "Echo headers and body from HTTP request."
--]]
local m = require("@lmb")

local t = {}
t.request = m.request
t.request.body = io.read("*a")
return t
