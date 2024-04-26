local m = require("@lam")

local t = {}
local total = 0
while true do
	local str = m:read(1024)
	if not str then
		print("read " .. total .. " byte(s)")
		break
	end
	total = total + #str
	for b in (str or ""):gmatch(".") do
		local k = tostring(string.byte(b))
		if t[k] == nil then
			t[k] = 1
		else
			t[k] = t[k] + 1
		end
	end
end

local keys = {}
for key in pairs(t) do
	table.insert(keys, key)
end
table.sort(keys, function(a, b)
	return t[a] > t[b]
end)
return t
