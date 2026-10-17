namespace App;

/// <summary>Ready is complete.</summary>
class Ready
{
    public void Ok() { }
}

class Broken
{
    public void M()
    {
        var s = "unterminated
    }
}

class Later
{
    public int x
