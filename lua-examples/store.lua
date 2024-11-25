--[[
--description = """
--Update an absent key 'a' in store and return the new value.
--Please note that since store is epheremal the output will always be 1.
--"""
--]]
local m = require("@lmb")
return m.store:update({ "a" }, function(values)
	local a = table.unpack(values)
	return table.pack(a + 1)
end, { 0 })
