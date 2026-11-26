Public Interface ICounter
    Function Current() As Integer
End Interface

Public Class Counter
    Implements ICounter

    Private items As Integer() = New Integer() {1, 2, 3}

    Public Function Current() As Integer Implements ICounter.Current
        Return items(0)
    End Function

    Public Overridable Function Name() As String
        Return "counter"
    End Function
End Class

Public Class Tally
    Inherits Counter

    Public Overrides Function Name() As String
        Return "tally"
    End Function
End Class
