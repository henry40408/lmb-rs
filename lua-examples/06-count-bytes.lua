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
		local bb = string.byte(b)
		if t[bb] == nil then
			t[bb] = 1
		else
			t[bb] = t[bb] + 1
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

local i = 0
for _, key in ipairs(keys) do
	if i > 10 then
		break
	end
	print(key, t[key])
	i = i + 1
end
