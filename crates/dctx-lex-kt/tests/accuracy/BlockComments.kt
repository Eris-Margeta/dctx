package comments

/* class PhantomBlock { fun phantomBlockFun() {} } */
class Visible {
    /*
     * fun phantomStar() {}
     */
    fun shown() = 1

    /* outer /* nested class PhantomNested */ still a comment: fun phantomStillComment() {} */
    fun afterNested() = 2

    // fun phantomLineComment() {}
    val kept = 3 // class PhantomTrailing
}

/* unfinished-looking "string inside comment
   interface PhantomQuoteInComment */
interface AfterComment

fun trailing() = 0 /* object PhantomEndComment */
