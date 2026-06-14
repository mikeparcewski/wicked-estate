using System;
using System.Collections.Generic;

namespace CodeIntel.Processing
{
    public interface IFormatter
    {
        string Format(string input);
    }

    public enum ProcessingMode
    {
        Batch,
        Streaming,
    }

    public class TextProcessor : IFormatter
    {
        public const int MaxLength = 10000;
        private readonly List<string> _log = new();
        private int _callCount;

        public string Format(string input)
        {
            var trimmed = Trim(input);
            Log(trimmed);
            return trimmed;
        }

        private string Trim(string s)
        {
            return s.Trim();
        }

        private void Log(string msg)
        {
            _log.Add(msg);
        }

        public int LogCount()
        {
            return _log.Count;
        }
    }

    public class Pipeline
    {
        private readonly IFormatter _formatter;

        public Pipeline(IFormatter formatter)
        {
            _formatter = formatter;
        }

        public string Run(string data)
        {
            return _formatter.Format(data);
        }
    }
}
