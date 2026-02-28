import sqlite3

def test_fts5():
    conn = sqlite3.connect(":memory:")
    try:
        conn.execute("CREATE VIRTUAL TABLE t USING fts5(x, tokenize='trigram')")
        print("FTS5 Trigram is supported!")
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    test_fts5()
