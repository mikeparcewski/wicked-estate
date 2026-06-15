function result = add(a, b)
    result = a + b;
end

function result = multiply(a, b)
    result = a * b;
end

function display_result(val)
    fprintf('Result: %g\n', val);
end

classdef Counter
    properties
        Value = 0
    end
    methods
        function obj = Counter(init)
            if nargin > 0
                obj.Value = init;
            end
        end
        function obj = increment(obj)
            obj.Value = obj.Value + 1;
        end
        function v = get_value(obj)
            v = obj.Value;
        end
    end
end
