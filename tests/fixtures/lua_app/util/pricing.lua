local M = {}

function M.calc_total(items)
  local tmp = 0
  for _, item in ipairs(items) do
    tmp = tmp + item.price
  end
  return tmp
end

return M
