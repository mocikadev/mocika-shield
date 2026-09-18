import { execFileSync } from "node:child_process";

function literal(value) {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return String(value);
  return `'${String(value).replaceAll("'", "''")}'`;
}

function expand(sql, values) {
  let index = 0;
  return sql.replaceAll("?", () => literal(values[index++]));
}

class SqliteStatement {
  constructor(db, sql, values = []) { this.db = db; this.sql = sql; this.values = values; }
  bind(...values) { return new SqliteStatement(this.db, this.sql, values); }
  async all() { return { results: this.db.query(expand(this.sql, this.values)) }; }
  async first() { return this.db.query(expand(this.sql, this.values))[0] ?? null; }
  async run() { this.db.exec(expand(this.sql, this.values)); return { success: true }; }
}

export class SqliteD1 {
  constructor(path) { this.path = path; }
  prepare(sql) { return new SqliteStatement(this, sql); }
  exec(sql) {
    execFileSync("sqlite3", ["-bail", this.path], { input: sql, stdio: ["pipe", "pipe", "pipe"] });
  }
  query(sql) {
    const output = execFileSync("sqlite3", ["-json", this.path], { input: sql, encoding: "utf8" }).trim();
    return output ? JSON.parse(output) : [];
  }
  queryPlan(sql) {
    return execFileSync("sqlite3", [this.path], { input: `EXPLAIN QUERY PLAN ${sql}`, encoding: "utf8" });
  }
  async batch(statements) {
    this.exec(`BEGIN IMMEDIATE;\n${statements.map((item) => `${expand(item.sql, item.values)};`).join("\n")}\nCOMMIT;`);
    return statements.map(() => ({ success: true }));
  }
}
