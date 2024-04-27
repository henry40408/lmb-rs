--[[
--description = "Echo the standard input."
--]]

local m = require("@lam")
print("Input: " .. m:read("*a"))
