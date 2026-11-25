package zoo

interface Feeder {
    fun feed(n: Int): Int
}

open class Keeper {
    open fun shift(): String = "day"
}

class Night : Keeper(), Feeder {
    override fun shift(): String = "night"
    override fun feed(n: Int): Int = n + 1
}

fun hire(): Keeper {
    val k = Night()
    k.feed(1)
    return k
}
