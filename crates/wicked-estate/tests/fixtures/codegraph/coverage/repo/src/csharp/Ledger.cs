namespace Cov
{
    public interface IEntry
    {
        decimal Amount();
    }

    public record Money(decimal Value);

    public abstract class Entry : IEntry
    {
        public abstract decimal Amount();
        public virtual string Label() { return "entry"; }
    }

    public class Credit : Entry
    {
        private Money held = new Money(1m);
        public override decimal Amount() { return held.Value; }
        public override string Label() { return "credit"; }
        public static Credit Open() { return new Credit(); }
    }
}
