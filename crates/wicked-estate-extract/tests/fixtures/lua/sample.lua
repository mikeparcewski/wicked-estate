local M = {}

local MAX_RETRIES = 3
local DEFAULT_DELAY = 0.1

local function clamp(value, min_val, max_val)
    if value < min_val then return min_val end
    if value > max_val then return max_val end
    return value
end

function M.retry(fn, attempts)
    attempts = clamp(attempts or MAX_RETRIES, 1, 10)
    for i = 1, attempts do
        local ok, result = pcall(fn)
        if ok then return result end
        if i < attempts then
            os.execute("sleep " .. DEFAULT_DELAY)
        end
    end
    error("all attempts failed")
end

function M.map(tbl, fn)
    local result = {}
    for i, v in ipairs(tbl) do
        result[i] = fn(v)
    end
    return result
end

function M.filter(tbl, pred)
    local result = {}
    for _, v in ipairs(tbl) do
        if pred(v) then
            result[#result + 1] = v
        end
    end
    return result
end

function M.pipeline(items, transforms)
    local out = items
    for _, fn in ipairs(transforms) do
        out = M.map(out, fn)
    end
    return out
end

return M
