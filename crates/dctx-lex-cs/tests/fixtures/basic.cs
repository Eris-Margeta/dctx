/// <summary>Sample types for café.</summary>
global using System.Linq;
using System.Collections.Generic;
using static System.Math;
using Alias = System.Collections.Generic.List<int>;
#region sample
namespace App.Sample
{
    /// <summary>A box of values.</summary>
    public partial class Box<T> : Base, IDisposable where T : class
    {
        /// <summary>Capacity.</summary>
        public int capacity;
        private List<string> names;
        public string Name { get; set; }
        /// <summary>Builds a box.</summary>
        public Box(int capacity) { store.Load(capacity); }
        public void Run()
        {
            service.Run();
            names.ForEach(item => sink.Accept(item));
            Abs(1);
            Point made = new Point(1, "a");
            string label = $"class Fake {sink.Flush()}";
            string s = "class Decoy {}";
            string v = @"class Hidden {}";
            string r = """
                class Raw {}
                """;
        }
#if DEBUG
        public void DebugOnly() { Trace(); }
#endif
        public static class Nested { void Ping() { pingBack(); } }
    }
    /// <summary>One point.</summary>
    public record Point(int X, string Label)
    {
        public int Doubled() => X;
    }
    public enum Color { Red, Green, Blue }
    public interface ITask { void Run(); }
    [Obsolete] public struct Holder { public int Value; }
}
#endregion
