--[[
--description = "Hash data with HMAC-SHA256."
--]]
local crypto = require("@lmb/crypto")
local s = io.read("*a")
return string.format("%s %s", crypto:hmac("sha256", s, "secret"), s)
