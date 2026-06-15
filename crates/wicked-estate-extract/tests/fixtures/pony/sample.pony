actor Main
  new create(env: Env) =>
    let c = Counter(0)
    c.increment()
    c.print_value(env)

class Counter
  var _value: I64

  new create(initial: I64) =>
    _value = initial

  fun ref increment() =>
    _value = _value + 1

  fun ref add(n: I64) =>
    _value = _value + n

  fun print_value(env: Env) =>
    env.out.print(_value.string())
