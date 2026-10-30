// Calc formula engine rebuild (LibreOffice-compatible semantics)
#include <bits/stdc++.h>
#include <zip.h>
#include <pugixml.hpp>
#include <pthread.h>
#include <charconv>
using namespace std;

enum { E_NULL = 1, E_DIV0, E_VALUE, E_REF, E_NAME, E_NUM, E_NA };
static const char* errStr(int e) {
  switch (e) { case E_NULL: return "#NULL!"; case E_DIV0: return "#DIV/0!"; case E_VALUE: return "#VALUE!";
    case E_REF: return "#REF!"; case E_NAME: return "#NAME?"; case E_NUM: return "#NUM!"; default: return "#N/A"; }
}
enum VT : uint8_t { V_EMPTY, V_NUM, V_STR, V_ERR, V_REF };
struct Val {
  VT t = V_EMPTY; bool isb = false; bool sw = false; int e = 0; double n = 0; string s;
  int sh = 0, r1 = 0, c1 = 0, r2 = 0, c2 = 0;
};
static inline Val mkNum(double d) { Val v; v.t = V_NUM; v.n = d; return v; }
static inline Val mkBool(bool b) { Val v; v.t = V_NUM; v.n = b ? 1 : 0; v.isb = true; return v; }
static inline Val mkErr(int e) { Val v; v.t = V_ERR; v.e = e; return v; }
static inline Val mkStr(string s) { Val v; v.t = V_STR; v.s = std::move(s); return v; }
static inline Val mkRef(int sh, int r1, int c1, int r2, int c2) { Val v; v.t = V_REF; v.sh = sh; v.r1 = r1; v.c1 = c1; v.r2 = r2; v.c2 = c2; return v; }

enum NT : uint8_t { N_NUM, N_STR, N_BOOL, N_ERR, N_REF, N_NEG, N_PCT, N_BIN, N_FUNC, N_MISS, N_POS };
struct Node;
struct NVec {
  Node* in[3]; Node** p = in; uint32_t n = 0, cap = 3;
  void push_back(Node* x) { if (n == cap) { Node** q = (Node**)malloc(sizeof(Node*) * cap * 2); memcpy(q, p, n * sizeof(Node*)); p = q; cap *= 2; } p[n++] = x; }
  NVec& operator=(std::initializer_list<Node*> l) { n = 0; for (auto x : l) push_back(x); return *this; }
  size_t size() const { return n; }
  Node*& operator[](size_t i) { return p[i]; }
  Node* const& operator[](size_t i) const { return p[i]; }
  Node** begin() { return p; } Node** end() { return p + n; }
  bool empty() const { return n == 0; }
};
struct Node {
  NT t; int op = 0; int fid = 0; double n = 0; string s; int sh = 0, r1 = 0, c1 = 0, r2 = 0, c2 = 0; NVec a;
};
static thread_local char* arenaP = nullptr; static thread_local size_t arenaLeft = 0;
static Node* newNode() {
  if (arenaLeft < sizeof(Node)) { size_t B = 1 << 20; arenaP = (char*)malloc(B); arenaLeft = B; }
  Node* n = new (arenaP) Node(); size_t sz = (sizeof(Node) + 15) & ~(size_t)15; arenaP += sz; arenaLeft = arenaLeft > sz ? arenaLeft - sz : 0; return n;
}
struct Cell { VT t = V_EMPTY; bool isb = false; int e = 0; double n = 0; string s; Node* f = nullptr; uint8_t st = 0; string ftext; };
struct Sheet { string name; int nr = 0, nc = 0; vector<Cell> cells; };
static vector<Sheet> sheets;
static Cell emptyCell;
static inline Cell* getCell(int sh, int r, int c) {
  Sheet& S = sheets[sh];
  if (r < 0 || c < 0 || r >= S.nr || c >= S.nc) return &emptyCell;
  return &S.cells[(size_t)r * S.nc + c];
}
static int curSh = 0, curR = 0, curC = 0; static bool curFmtBool = false;
struct ArgCache { Node* fn; Val* vals; };
static ArgCache* gCache = nullptr;

// ---------------- number <-> string ----------------
static string fmtNum(double v) {
  if (v == 0) return "0";
  if (!std::isfinite(v)) return "#NUM!";
  char buf[64];
  int p = 15;
  for (; p <= 17; p++) { snprintf(buf, sizeof buf, "%.*e", p - 1, v); if (strtod(buf, nullptr) == v) break; }
  if (p > 17) p = 17;
  // extract digits and exponent
  bool neg = v < 0; const char* q = buf; if (*q == '-') q++;
  string dg; while (*q && *q != 'e') { if (isdigit((unsigned char)*q)) dg += *q; q++; }
  int ex = atoi(q + 1);
  if ((int)dg.size() > 15) {
    bool up = dg[15] >= '5';
    dg.resize(15);
    if (up) { int i = 14; while (i >= 0) { if (dg[i] == '9') { dg[i] = '0'; i--; } else { dg[i]++; break; } } if (i < 0) { dg = "1" + dg; dg.resize(15); ex++; } }
  }
  while (dg.size() > 1 && dg.back() == '0') dg.pop_back();
  string out = neg ? "-" : "";
  if (ex >= 16 || ex <= -15) {
    out += dg[0]; if (dg.size() > 1) { out += '.'; out += dg.substr(1); }
    char eb[16]; snprintf(eb, sizeof eb, "E%c%03d", ex < 0 ? '-' : '+', abs(ex));
    return out + eb;
  }
  if (ex < 0) { out += "0."; out += string(-ex - 1, '0'); out += dg; return out; }
  if ((int)dg.size() <= ex + 1) { out += dg; out += string(ex + 1 - dg.size(), '0'); return out; }
  out += dg.substr(0, ex + 1); out += '.'; out += dg.substr(ex + 1);
  return out;
}
static long long daysFromCivil(long long y, unsigned m, unsigned d) {
  y -= m <= 2; const long long era = (y >= 0 ? y : y - 399) / 400; const unsigned yoe = (unsigned)(y - era * 400);
  const unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1; const unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  return era * 146097 + (long long)doe - 719468;
}
static void civilFromDays(long long z, int& y, int& m, int& d) {
  z += 719468; const long long era = (z >= 0 ? z : z - 146096) / 146097; const unsigned doe = (unsigned)(z - era * 146097);
  const unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; const long long yy = (long long)yoe + era * 400;
  const unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100); const unsigned mp = (5 * doy + 2) / 153;
  d = doy - (153 * mp + 2) / 5 + 1; m = mp < 10 ? mp + 3 : mp - 9; y = (int)(yy + (m <= 2));
}
static const long long BASE = daysFromCivil(1899, 12, 30);
static double serialFromYMD(long long y, long long m, long long d) {
  // normalize month
  long long mm = m - 1; y += mm >= 0 ? mm / 12 : -((-mm + 11) / 12); mm = ((mm % 12) + 12) % 12;
  return (double)(daysFromCivil(y, (unsigned)(mm + 1), 1) - BASE + (d - 1));
}
static void ymdFromSerial(double s, int& y, int& m, int& d) { civilFromDays((long long)floor(s) + BASE, y, m, d); }
static bool isLeap(int y) { return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0; }
static int daysInMonth(int y, int m) { static int dm[] = {31,28,31,30,31,30,31,31,30,31,30,31}; return m == 2 && isLeap(y) ? 29 : dm[m - 1]; }

static bool parseNumStr(const string& s0, double& out) {
  size_t b = 0, e = s0.size();
  while (b < e && isspace((unsigned char)s0[b])) b++;
  while (e > b && isspace((unsigned char)s0[e - 1])) e--;
  if (b >= e) return false;
  string s = s0.substr(b, e - b);
  if (s.size() == 4 || s.size() == 5) { string u = s; for (auto& ch : u) ch = toupper((unsigned char)ch); if (u == "TRUE") { out = 1; return true; } if (u == "FALSE") { out = 0; return true; } }
  // plain number
  {
    const char* p = s.c_str(); char* end;
    size_t i = 0; if (s[i] == '+' || s[i] == '-') i++;
    bool dig = false; while (i < s.size() && isdigit((unsigned char)s[i])) { i++; dig = true; }
    if (i < s.size() && s[i] == '.') { i++; while (i < s.size() && isdigit((unsigned char)s[i])) { i++; dig = true; } }
    if (dig) {
      size_t j = i;
      if (j < s.size() && (s[j] == 'e' || s[j] == 'E')) { size_t k = j + 1; if (k < s.size() && (s[k] == '+' || s[k] == '-')) k++; bool ed = false; while (k < s.size() && isdigit((unsigned char)s[k])) { k++; ed = true; } if (ed) i = k; }
      if (i == s.size()) { out = strtod(p, &end); return true; }
      if (i == s.size() - 1 && s[i] == '%') { out = strtod(p, &end) / 100.0; return true; }
    }
  }
  if (s.find(',') != string::npos) {
    string t; size_t i = 0; if (s[0] == '-' || s[0] == '+') { t += s[0]; i = 1; }
    size_t dot = s.find('.'); size_t intEnd = dot == string::npos ? s.size() : dot;
    string ip = s.substr(i, intEnd - i); bool ok = !ip.empty() && ip[0] != ',';
    int grp = 0; bool seenComma = false; string digits;
    for (int k = (int)ip.size() - 1; k >= 0 && ok; k--) { char c = ip[k]; if (c == ',') { if (grp != 3) ok = false; grp = 0; seenComma = true; } else if (isdigit((unsigned char)c)) { grp++; digits = c + digits; } else ok = false; }
    if (ok && seenComma) { string rest = s.substr(intEnd); string cand = t + digits + rest; if (cand.find(',') == string::npos) { double d; if (parseNumStr(cand, d)) { out = d; return true; } } }
  }
  {
    static const char* mn[] = {"jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"};
    int d = 0, y = 0; char mb[16]; int n = 0;
    if (sscanf(s.c_str(), "%d-%3[A-Za-z]-%d%n", &d, mb, &y, &n) == 3 && n == (int)s.size()) {
      string m = mb; for (auto& ch : m) ch = tolower((unsigned char)ch);
      for (int k = 0; k < 12; k++) if (m == mn[k]) {
        size_t lastDash = s.rfind('-'); int ydig = (int)(s.size() - lastDash - 1);
        if (ydig <= 2) y += (y < 30 ? 2000 : 1900);
        if (d >= 1 && d <= daysInMonth(y, k + 1)) { out = serialFromYMD(y, k + 1, d); return true; }
      }
    }
  }
  {
    int hh, mm, ss = 0, n = 0, n2 = 0;
    if (sscanf(s.c_str(), "%d:%d%n:%d%n", &hh, &mm, &n, &ss, &n2) >= 2) {
      int used = n2 ? n2 : n;
      if (used == (int)s.size() && hh >= 0 && mm >= 0 && mm < 60 && ss >= 0 && ss < 60) { out = (hh * 3600.0 + mm * 60.0 + ss) / 86400.0; return true; }
    }
  }
  if (s[0] == '$' || (s.size() > 2 && s[0] == '-' && s[1] == '$')) {
    string t = s[0] == '$' ? s.substr(1) : "-" + s.substr(2);
    if (!t.empty() && t[0] != '$' && t.find('$') == string::npos) { double d; if (parseNumStr(t, d)) { out = d; return true; } }
  }
  {
    static const char* mfull[] = {"january","february","march","april","may","june","july","august","september","october","november","december"};
    char mb[32]; int y = 0, n = 0;
    if (s.size() < 30 && sscanf(s.c_str(), "%31[A-Za-z] %d%n", mb, &y, &n) == 2 && n == (int)s.size()) {
      string m = mb; for (auto& ch : m) ch = tolower((unsigned char)ch);
      for (int k = 0; k < 12; k++) if (m == mfull[k] || (m.size() == 3 && m == string(mfull[k]).substr(0, 3))) {
        size_t sp = s.rfind(' '); int ydig = (int)(s.size() - sp - 1);
        if (ydig <= 2) y += (y < 30 ? 2000 : 1900);
        out = serialFromYMD(y, k + 1, 1); return true;
      }
    }
  }
  // ISO date yyyy-mm-dd
  {
    int y, m, d; char c1, c2; int n = 0;
    if (sscanf(s.c_str(), "%d%c%d%c%d%n", &y, &c1, &m, &c2, &d, &n) == 5 && n == (int)s.size() && c1 == c2 && (c1 == '-' || c1 == '/')) {
      if (c1 == '-' && y >= 1000 && m >= 1 && m <= 12 && d >= 1 && d <= daysInMonth(y, m)) { out = serialFromYMD(y, m, d); return true; }
      if (c1 == '/' && y >= 1 && y <= 12 && m >= 1 && m <= 31) { // m/d/y
        int yy = d; if (yy < 100) yy += (yy < 30 ? 2000 : 1900);
        if (m <= daysInMonth(yy, y)) { out = serialFromYMD(yy, y, m); return true; }
      }
    }
  }
  return false;
}

enum { FID_NONE = 0, FID_ABS, FID_AND, FID_AVERAGE, FID_AVERAGEIF, FID_AVERAGEIFS, FID_CHOOSE, FID_CONCAT, FID_CONCATENATE, FID_COUNT, FID_COUNTA, FID_COUNTBLANK, FID_COUNTIF, FID_COUNTIFS, FID_DATE, FID_DATEDIF, FID_DAY, FID_DAYS, FID_EDATE, FID_EOMONTH, FID_EXACT, FID_EXP, FID_FALSE, FID_FIND, FID_HLOOKUP, FID_IF, FID_IFERROR, FID_IFNA, FID_IFS, FID_INDEX, FID_INT, FID_ISBLANK, FID_ISERROR, FID_ISLOGICAL, FID_ISNA, FID_ISNUMBER, FID_ISTEXT, FID_LARGE, FID_LEFT, FID_LEN, FID_LN, FID_LOG, FID_LOG10, FID_LOWER, FID_MATCH, FID_MAX, FID_MEDIAN, FID_MID, FID_MIN, FID_MOD, FID_MONTH, FID_N, FID_NOT, FID_OR, FID_PI, FID_POWER, FID_PRODUCT, FID_PROPER, FID_REPLACE, FID_REPT, FID_RIGHT, FID_ROUND, FID_ROUNDDOWN, FID_ROUNDUP, FID_SEARCH, FID_SIGN, FID_SMALL, FID_SQRT, FID_STDEV, FID_SUBSTITUTE, FID_SUM, FID_SUMIF, FID_SUMIFS, FID_SUMPRODUCT, FID_SUMSQ, FID_SWITCH, FID_T, FID_TEXT, FID_TEXTJOIN, FID_TRIM, FID_TRUE, FID_TRUNC, FID_UPPER, FID_VALUE, FID_VAR, FID_VLOOKUP, FID_WEEKDAY, FID_XLOOKUP, FID_XOR, FID_YEAR };
static unordered_map<string,int> fidMap = {{"ABS", FID_ABS}, {"AND", FID_AND}, {"AVERAGE", FID_AVERAGE}, {"AVERAGEIF", FID_AVERAGEIF}, {"AVERAGEIFS", FID_AVERAGEIFS}, {"CHOOSE", FID_CHOOSE}, {"CONCAT", FID_CONCAT}, {"CONCATENATE", FID_CONCATENATE}, {"COUNT", FID_COUNT}, {"COUNTA", FID_COUNTA}, {"COUNTBLANK", FID_COUNTBLANK}, {"COUNTIF", FID_COUNTIF}, {"COUNTIFS", FID_COUNTIFS}, {"DATE", FID_DATE}, {"DATEDIF", FID_DATEDIF}, {"DAY", FID_DAY}, {"DAYS", FID_DAYS}, {"EDATE", FID_EDATE}, {"EOMONTH", FID_EOMONTH}, {"EXACT", FID_EXACT}, {"EXP", FID_EXP}, {"FALSE", FID_FALSE}, {"FIND", FID_FIND}, {"HLOOKUP", FID_HLOOKUP}, {"IF", FID_IF}, {"IFERROR", FID_IFERROR}, {"IFNA", FID_IFNA}, {"IFS", FID_IFS}, {"INDEX", FID_INDEX}, {"INT", FID_INT}, {"ISBLANK", FID_ISBLANK}, {"ISERROR", FID_ISERROR}, {"ISLOGICAL", FID_ISLOGICAL}, {"ISNA", FID_ISNA}, {"ISNUMBER", FID_ISNUMBER}, {"ISTEXT", FID_ISTEXT}, {"LARGE", FID_LARGE}, {"LEFT", FID_LEFT}, {"LEN", FID_LEN}, {"LN", FID_LN}, {"LOG", FID_LOG}, {"LOG10", FID_LOG10}, {"LOWER", FID_LOWER}, {"MATCH", FID_MATCH}, {"MAX", FID_MAX}, {"MEDIAN", FID_MEDIAN}, {"MID", FID_MID}, {"MIN", FID_MIN}, {"MOD", FID_MOD}, {"MONTH", FID_MONTH}, {"N", FID_N}, {"NOT", FID_NOT}, {"OR", FID_OR}, {"PI", FID_PI}, {"POWER", FID_POWER}, {"PRODUCT", FID_PRODUCT}, {"PROPER", FID_PROPER}, {"REPLACE", FID_REPLACE}, {"REPT", FID_REPT}, {"RIGHT", FID_RIGHT}, {"ROUND", FID_ROUND}, {"ROUNDDOWN", FID_ROUNDDOWN}, {"ROUNDUP", FID_ROUNDUP}, {"SEARCH", FID_SEARCH}, {"SIGN", FID_SIGN}, {"SMALL", FID_SMALL}, {"SQRT", FID_SQRT}, {"STDEV", FID_STDEV}, {"SUBSTITUTE", FID_SUBSTITUTE}, {"SUM", FID_SUM}, {"SUMIF", FID_SUMIF}, {"SUMIFS", FID_SUMIFS}, {"SUMPRODUCT", FID_SUMPRODUCT}, {"SUMSQ", FID_SUMSQ}, {"SWITCH", FID_SWITCH}, {"T", FID_T}, {"TEXT", FID_TEXT}, {"TEXTJOIN", FID_TEXTJOIN}, {"TRIM", FID_TRIM}, {"TRUE", FID_TRUE}, {"TRUNC", FID_TRUNC}, {"UPPER", FID_UPPER}, {"VALUE", FID_VALUE}, {"VAR", FID_VAR}, {"VLOOKUP", FID_VLOOKUP}, {"WEEKDAY", FID_WEEKDAY}, {"XLOOKUP", FID_XLOOKUP}, {"XOR", FID_XOR}, {"YEAR", FID_YEAR}};
// ---------------- parser ----------------
static unordered_map<string, int> sheetIdx;
struct Parser {
  const string& s; size_t p = 0; int psh;
  Parser(const string& str, int sh) : s(str), psh(sh) {}
  void ws() { while (p < s.size() && s[p] == ' ') p++; }
  bool eat(char c) { ws(); if (p < s.size() && s[p] == c) { p++; return true; } return false; }
  Node* nn(NT t) { Node* n = newNode(); n->t = t; return n; }
  Node* expr() { return cmp(); }
  Node* cmp() {
    Node* l = cat();
    for (;;) {
      ws(); int op = 0;
      if (s.compare(p, 2, "<>") == 0) { op = 'N'; p += 2; }
      else if (s.compare(p, 2, "<=") == 0) { op = 'L'; p += 2; }
      else if (s.compare(p, 2, ">=") == 0) { op = 'G'; p += 2; }
      else if (p < s.size() && (s[p] == '<' || s[p] == '>' || s[p] == '=')) { op = s[p]; p++; }
      else break;
      Node* r = cat(); Node* n = nn(N_BIN); n->op = op; n->a = {l, r}; l = n;
    }
    return l;
  }
  Node* cat() { Node* l = add(); while (eat('&')) { Node* r = add(); Node* n = nn(N_BIN); n->op = '&'; n->a = {l, r}; l = n; } return l; }
  Node* add() {
    Node* l = mul();
    for (;;) { ws(); if (p < s.size() && (s[p] == '+' || s[p] == '-')) { int op = s[p++]; Node* r = mul(); Node* n = nn(N_BIN); n->op = op; n->a = {l, r}; l = n; } else break; }
    return l;
  }
  Node* mul() {
    Node* l = pw();
    for (;;) { ws(); if (p < s.size() && (s[p] == '*' || s[p] == '/')) { int op = s[p++]; Node* r = pw(); Node* n = nn(N_BIN); n->op = op; n->a = {l, r}; l = n; } else break; }
    return l;
  }
  Node* pw() { Node* l = un(); while (eat('^')) { Node* r = un(); Node* n = nn(N_BIN); n->op = '^'; n->a = {l, r}; l = n; } return l; }
  Node* un() {
    ws();
    if (p < s.size() && s[p] == '-') { p++; Node* n = nn(N_NEG); n->a = {un()}; return n; }
    if (p < s.size() && s[p] == '+') { p++; Node* n = nn(N_POS); n->a = {un()}; return n; }
    return post();
  }
  Node* post() { Node* l = prim(); while (eat('%')) { Node* n = nn(N_PCT); n->a = {l}; l = n; } return l; }
  static bool parseCellRef(const string& s, size_t& p, int& r, int& c) {
    size_t q = p; if (q < s.size() && s[q] == '$') q++;
    int col = 0; size_t cs = q; while (q < s.size() && isalpha((unsigned char)s[q])) { col = col * 26 + (toupper(s[q]) - 'A' + 1); q++; }
    if (q == cs || q - cs > 3) return false;
    if (q < s.size() && s[q] == '$') q++;
    size_t rs = q; long row = 0; while (q < s.size() && isdigit((unsigned char)s[q])) { row = row * 10 + (s[q] - '0'); q++; }
    if (q == rs) return false;
    if (q < s.size() && (isalnum((unsigned char)s[q]) || s[q] == '_' || s[q] == '(')) return false;
    r = (int)row - 1; c = col - 1; p = q; return true;
  }
  Node* prim() {
    ws();
    if (p >= s.size()) return nn(N_MISS);
    char ch = s[p];
    if (ch == '(') { p++; Node* e = expr(); eat(')'); return e; }
    if (ch == '"') {
      p++; string str;
      while (p < s.size()) { if (s[p] == '"') { if (p + 1 < s.size() && s[p + 1] == '"') { str += '"'; p += 2; } else { p++; break; } } else str += s[p++]; }
      Node* n = nn(N_STR); n->s = str; return n;
    }
    if (isdigit((unsigned char)ch) || (ch == '.' && p + 1 < s.size() && isdigit((unsigned char)s[p + 1]))) {
      size_t q = p; while (q < s.size() && (isdigit((unsigned char)s[q]) || s[q] == '.')) q++;
      if (q < s.size() && (s[q] == 'e' || s[q] == 'E')) { size_t k = q + 1; if (k < s.size() && (s[k] == '+' || s[k] == '-')) k++; if (k < s.size() && isdigit((unsigned char)s[k])) { while (k < s.size() && isdigit((unsigned char)s[k])) k++; q = k; } }
      Node* n = nn(N_NUM); n->n = strtod(s.substr(p, q - p).c_str(), nullptr); p = q; return n;
    }
    if (ch == '#') {
      static const char* es[] = {"#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"};
      for (int i = 0; i < 7; i++) { size_t L = strlen(es[i]); if (s.compare(p, L, es[i]) == 0) { p += L; Node* n = nn(N_ERR); n->op = i + 1; return n; } }
      p++; Node* n = nn(N_ERR); n->op = E_NAME; return n;
    }
    if (ch == ',' || ch == ')') return nn(N_MISS);
    // sheet-qualified ref
    int sh = psh; size_t save = p;
    if (ch == '\'') {
      size_t q = p + 1; string nm;
      while (q < s.size()) { if (s[q] == '\'') { if (q + 1 < s.size() && s[q + 1] == '\'') { nm += '\''; q += 2; } else { q++; break; } } else nm += s[q++]; }
      if (q < s.size() && s[q] == '!') { auto it = sheetIdx.find(nm); p = q + 1; if (it == sheetIdx.end()) { Node* n = nn(N_ERR); n->op = E_REF; int r, c; parseCellRef(s, p, r, c); if (p < s.size() && s[p] == ':') { p++; parseCellRef(s, p, r, c); } return n; } sh = it->second; return refAfter(sh); }
    }
    // identifier
    size_t q = p; while (q < s.size() && (isalnum((unsigned char)s[q]) || s[q] == '_' || s[q] == '.')) q++;
    string id = s.substr(p, q - p);
    if (q < s.size() && s[q] == '!') {
      auto it = sheetIdx.find(id); p = q + 1;
      if (it == sheetIdx.end()) { Node* n = nn(N_ERR); n->op = E_REF; int r, c; parseCellRef(s, p, r, c); if (p < s.size() && s[p] == ':') { p++; parseCellRef(s, p, r, c); } return n; }
      return refAfter(it->second);
    }
    {
      size_t pp = p; int r, c;
      if (parseCellRef(s, pp, r, c)) { return refAfter(sh); }
    }
    p = q; ws();
    if (p < s.size() && s[p] == '(') {
      p++;
      string nm = id; for (auto& c : nm) c = toupper(c);
      if (nm.rfind("_XLFN.", 0) == 0) nm = nm.substr(6);
      if (nm.rfind("_XLWS.", 0) == 0) nm = nm.substr(6);
      Node* n = nn(N_FUNC); n->s = nm; { auto it = fidMap.find(nm); n->fid = it == fidMap.end() ? 0 : it->second; }
      ws();
      if (p < s.size() && s[p] == ')') { p++; return n; }
      for (;;) {
        Node* a = expr(); n->a.push_back(a);
        ws();
        if (p < s.size() && s[p] == ',') { p++; continue; }
        if (p < s.size() && s[p] == ')') { p++; break; }
        break;
      }
      return n;
    }
    string up = id; for (auto& c : up) c = toupper(c);
    if (up == "TRUE") { Node* n = nn(N_BOOL); n->n = 1; return n; }
    if (up == "FALSE") { Node* n = nn(N_BOOL); n->n = 0; return n; }
    (void)save;
    Node* n = nn(N_ERR); n->op = E_NAME; return n;
  }
  Node* refAfter(int sh) {
    int r1, c1, r2, c2;
    if (!parseCellRef(s, p, r1, c1)) { Node* n = nn(N_ERR); n->op = E_REF; return n; }
    r2 = r1; c2 = c1;
    if (p < s.size() && s[p] == ':') {
      size_t q = p + 1; int r, c;
      if (parseCellRef(s, q, r, c)) { p = q; r2 = r; c2 = c; }
    }
    Node* n = nn(N_REF); n->sh = sh; n->r1 = min(r1, r2); n->r2 = max(r1, r2); n->c1 = min(c1, c2); n->c2 = max(c1, c2); return n;
  }
};

// ---------------- evaluation ----------------
static Val eval(Node* n);
static void computeCell(Cell* c, int sh, int r, int col);
static inline Cell* cellAt(int sh, int r, int c) {
  Cell* cl = getCell(sh, r, c);
  if (cl->f && cl->st != 2) computeCell(cl, sh, r, c);
  return cl;
}
static Val cellVal(int sh, int r, int c) {
  Cell* cl = cellAt(sh, r, c);
  Val v; v.t = cl->t; v.isb = cl->isb; v.e = cl->e; v.n = cl->n; if (cl->t == V_STR) v.s = cl->s;
  return v;
}
static Val deref(const Val& v) {
  if (v.t != V_REF) return v;
  if (v.r1 == v.r2 && v.c1 == v.c2) return cellVal(v.sh, v.r1, v.c1);
  if (v.c1 == v.c2 && curR >= v.r1 && curR <= v.r2) return cellVal(v.sh, curR, v.c1);
  if (v.r1 == v.r2 && curC >= v.c1 && curC <= v.c2) return cellVal(v.sh, v.r1, curC);
  return mkErr(E_VALUE);
}
static inline Val sc(Node* n) { Val v = eval(n); if (v.t == V_REF) return deref(v); return v; }
static bool toNum(const Val& v, double& d, int& err) {
  switch (v.t) {
    case V_NUM: d = v.n; return true;
    case V_EMPTY: d = 0; return true;
    case V_ERR: err = v.e; return false;
    case V_STR: if (parseNumStr(v.s, d)) return true; err = E_VALUE; return false;
    default: err = E_VALUE; return false;
  }
}
static string toStr(const Val& v) {
  switch (v.t) {
    case V_NUM: return fmtNum(v.n);
    case V_STR: return v.s;
    default: return "";
  }
}
static string lower(const string& s) { string r = s; for (auto& c : r) c = tolower((unsigned char)c); return r; }
static string upper(const string& s) { string r = s; for (auto& c : r) c = toupper((unsigned char)c); return r; }
static int cmpStrCI(const string& a, const string& b) {
  size_t n = min(a.size(), b.size());
  for (size_t i = 0; i < n; i++) { int x = tolower((unsigned char)a[i]), y = tolower((unsigned char)b[i]); if (x != y) return x < y ? -1 : 1; }
  return a.size() < b.size() ? -1 : a.size() > b.size() ? 1 : 0;
}
// compare two scalar values: returns -1,0,1 ; types: empty, num, str
static int cmpVals(const Val& a, const Val& b) {
  int ta = a.t == V_STR ? 1 : 0, tb = b.t == V_STR ? 1 : 0;
  if (a.t == V_EMPTY && b.t == V_STR) ta = 1;
  if (b.t == V_EMPTY && a.t == V_STR) tb = 1;
  if (ta != tb) return ta < tb ? -1 : 1;
  if (ta == 1) { return cmpStrCI(a.t == V_STR ? a.s : string(), b.t == V_STR ? b.s : string()); }
  double x = a.t == V_NUM ? a.n : 0, y = b.t == V_NUM ? b.n : 0;
  return x < y ? -1 : x > y ? 1 : 0;
}

// wildcard match, case-insensitive
static bool wildMatch(const char* p, const char* pe, const char* s, const char* se) {
  while (p < pe) {
    char c = *p;
    if (c == '*') { p++; while (p < pe && *p == '*') p++; if (p == pe) return true; for (const char* t = s; t <= se; t++) if (wildMatch(p, pe, t, se)) return true; return false; }
    if (s >= se) return false;
    if (c == '?') { p++; s++; continue; }
    if (c == '~' && p + 1 < pe && (p[1] == '*' || p[1] == '?' || p[1] == '~')) { p++; c = *p; }
    if (tolower((unsigned char)c) != tolower((unsigned char)*s)) return false;
    p++; s++;
  }
  return s == se;
}
static bool hasWild(const string& s) { return s.find_first_of("*?~") != string::npos; }
static bool strEqWild(const string& pat, const string& s, bool wild) {
  if (!wild) return cmpStrCI(pat, s) == 0;
  return wildMatch(pat.data(), pat.data() + pat.size(), s.data(), s.data() + s.size());
}

struct Crit {
  int op = 0; // 0 =,1 <>,2 <,3 <=,4 >,5 >=
  bool isNum = false; double n = 0; string s; bool wild = false; bool empty = false;
};
static Crit makeCrit(const Val& v) {
  Crit c;
  if (v.t == V_NUM) { c.isNum = true; c.n = v.n; return c; }
  if (v.t == V_EMPTY) { c.isNum = true; c.n = 0; return c; }
  string s = v.s; size_t i = 0;
  if (s.compare(0, 2, "<>") == 0) { c.op = 1; i = 2; }
  else if (s.compare(0, 2, "<=") == 0) { c.op = 3; i = 2; }
  else if (s.compare(0, 2, ">=") == 0) { c.op = 5; i = 2; }
  else if (s.compare(0, 1, "<") == 0) { c.op = 2; i = 1; }
  else if (s.compare(0, 1, ">") == 0) { c.op = 4; i = 1; }
  else if (s.compare(0, 1, "=") == 0) { c.op = 0; i = 1; }
  string rest = s.substr(i);
  if (rest.empty()) { c.empty = true; return c; }
  double d;
  if (parseNumStr(rest, d)) { c.isNum = true; c.n = d; c.s = rest; return c; }
  c.s = rest; c.wild = (c.op <= 1) && hasWild(rest);
  return c;
}
static inline bool cmpOp(int op, int r) {
  switch (op) { case 0: return r == 0; case 1: return r != 0; case 2: return r < 0; case 3: return r <= 0; case 4: return r > 0; default: return r >= 0; }
}
static bool critMatch(const Crit& c, const Cell* cl) {
  VT t = cl->t;
  if (c.empty) {
    bool isEmpty = (t == V_EMPTY) || (t == V_STR && cl->s.empty());
    if (c.op == 0) return isEmpty;
    if (c.op == 1) return !isEmpty;
    if (c.op == 4) return t == V_STR && !cl->s.empty();
    if (c.op == 5) return t == V_STR;
    return false;
  }
  if (t == V_ERR) return false;
  if (c.isNum) {
    if (t == V_NUM) { int r = cl->n < c.n ? -1 : cl->n > c.n ? 1 : 0; return cmpOp(c.op, r); }
    if (t == V_STR && !c.s.empty()) {
      if (c.op == 0) return cmpStrCI(cl->s, c.s) == 0;
      if (c.op == 1) return cmpStrCI(cl->s, c.s) != 0;
      return false;
    }
    return c.op == 1;
  }
  if (t == V_STR) {
    if (c.op == 0) return strEqWild(c.s, cl->s, c.wild);
    if (c.op == 1) return !strEqWild(c.s, cl->s, c.wild);
    return cmpOp(c.op, cmpStrCI(cl->s, c.s));
  }
  if (t == V_NUM && c.wild) {
    string ns = fmtNum(cl->n);
    bool m = strEqWild(c.s, ns, true);
    return c.op == 0 ? m : !m;
  }
  return c.op == 1;
}

static void computeCell(Cell* c, int sh, int r, int col) {
  if (c->st == 1) { c->t = V_ERR; c->e = E_VALUE; return; } // circular
  c->st = 1;
  ArgCache* savedCache = gCache; gCache = nullptr;
  int os = curSh, orr = curR, oc = curC; bool ofb = curFmtBool; curFmtBool = false;
  curSh = sh; curR = r; curC = col;
  Val v = eval(c->f);
  if (v.t == V_REF) v = deref(v);
  if (v.t == V_EMPTY) { v.t = V_NUM; v.n = 0; }
  curSh = os; curR = orr; curC = oc; curFmtBool = ofb; gCache = savedCache;
  c->t = v.t; c->isb = v.isb; c->e = v.e; c->n = v.n; c->s = std::move(v.s);
  if (c->t == V_NUM && !std::isfinite(c->n)) { c->t = V_ERR; c->e = E_NUM; }
  c->st = 2;
}

// Iterate values of an argument: callback(Cell-like value, fromRange)
template <class F> static bool iterArg(Node* a, F&& f, int& err) {
  Val v = eval(a);
  if (v.t == V_REF) {
    for (int r = v.r1; r <= v.r2; r++)
      for (int c = v.c1; c <= v.c2; c++) {
        Cell* cl = cellAt(v.sh, r, c);
        if (!f(cl->t, cl->n, cl->e, cl->s, true, cl)) { return false; }
      }
    return true;
  }
  return f(v.t, v.n, v.e, v.s, false, (Cell*)nullptr);
}

// gather numbers per Calc rules: ranges -> numbers only; direct -> numbers/bools, strings -> #VALUE!
static bool gatherNums(Node* fn, vector<double>& out, int& err) {
  for (Node* a : fn->a) {
    if (a->t == N_MISS) continue;
    bool ok = iterArg(a, [&](VT t, double n, int e, const string& s, bool rng, Cell*) {
      if (t == V_ERR) { err = e; return false; }
      if (t == V_NUM) { out.push_back(n); return true; }
      if (!rng) {
        if (t == V_EMPTY) { out.push_back(0); return true; }
        if (t == V_STR) { err = E_VALUE; return false; }
      }
      return true;
    }, err);
    if (!ok) return false;
  }
  return true;
}

static double roundSig(double x) { // snap to 15 significant digits
  if (x == 0 || !std::isfinite(x)) return x;
  char buf[64]; snprintf(buf, sizeof buf, "%.15g", x); return strtod(buf, nullptr);
}
static double roundDigits(double x, int d, int mode) { // mode 0 round half away, 1 down(trunc), 2 up(away)
  double p = pow(10.0, d);
  double t = x * p;
  { double ft = floor(t), frac = t - ft; if (fabs(frac - 0.5) < 1e-7 || frac < 1e-7 || frac > 1 - 1e-7) t = roundSig(t); }
  double r;
  if (mode == 0) r = t < 0 ? -floor(-t + 0.5) : floor(t + 0.5);
  else if (mode == 1) r = t < 0 ? -floor(-t) : floor(t);
  else r = t < 0 ? -ceil(-t) : ceil(t);
  double res = r / p;
  if (d < 0) res = r * pow(10.0, -d);
  return res;
}

static Val powV(double x, double y) {
  if (x == 0 && y == 0) return mkNum(1);
  if (x == 0 && y < 0) return mkErr(E_NUM);
  double r;
  if (x < 0 && y != floor(y)) {
    double inv = 1.0 / y; double ri = round(inv);
    if (fabs(inv - ri) < 1e-10 && fmod(fabs(ri), 2.0) == 1.0) r = -pow(-x, y);
    else return mkErr(E_NUM);
  } else r = pow(x, y);
  if (!std::isfinite(r)) return mkErr(E_NUM);
  return mkNum(r);
}
struct KSum { double s = 0, c = 0; inline void add(double x) { double t = s + x; if (fabs(s) >= fabs(x)) c += (s - t) + x; else c += (x - t) + s; s = t; } inline double get() const { return s + c; } };
static inline bool approxEq(double a, double b) { if (a == b) return true; double d = fabs(a - b); return d < fabs(a) * (1.0 / (1LL << 48)) && d < fabs(b) * (1.0 / (1LL << 48)); }
static inline double approxAdd(double a, double b) { if (((a < 0 && b > 0) || (b < 0 && a > 0)) && approxEq(a, -b)) return 0.0; return a + b; }
static inline double approxSub(double a, double b) { if (((a < 0 && b < 0) || (a > 0 && b > 0)) && approxEq(a, b)) return 0.0; return a - b; }
// ---------------- TEXT() formatting ----------------
static const char* MON[] = {"January","February","March","April","May","June","July","August","September","October","November","December"};
static const char* DAYN[] = {"Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"};
static string groupInt(const string& digits) {
  string r; int n = digits.size();
  for (int i = 0; i < n; i++) { r += digits[i]; int left = n - 1 - i; if (left > 0 && left % 3 == 0) r += ','; }
  return r;
}
static string formatText(double v, const string& fmt0) {
  // split sections
  vector<string> secs; { string cur; bool q = false; for (char c : fmt0) { if (c == '"') q = !q; if (c == ';' && !q) { secs.push_back(cur); cur.clear(); } else cur += c; } secs.push_back(cur); }
  string fmt = secs[0]; bool negSec = false;
  if (v < 0 && secs.size() >= 2) { fmt = secs[1]; negSec = true; v = -v; }
  else if (v == 0 && secs.size() >= 3) fmt = secs[2];
  string uf = upper(fmt);
  if (uf == "GENERAL" || uf == "@") return fmtNum(v);
  // tokenise
  struct Tok { int k; string s; }; // k: 0 literal, 1 date code
  bool isDate = false; bool hasDigit = false;
  { bool q = false; for (size_t i = 0; i < fmt.size(); i++) { char c = fmt[i]; if (c == '"') { q = !q; continue; } if (q) continue; if (c == '\\') { i++; continue; } char l = tolower(c); if (l == 'y' || l == 'd' || l == 'h' || l == 's' || l == 'm') isDate = true; if (c == '0' || c == '#' || c == '?') hasDigit = true; } }
  if (isDate && !hasDigit) {
    vector<Tok> toks;
    for (size_t i = 0; i < fmt.size();) {
      char c = fmt[i];
      if (c == '"') { size_t j = fmt.find('"', i + 1); if (j == string::npos) j = fmt.size(); toks.push_back({0, fmt.substr(i + 1, j - i - 1)}); i = j + 1; continue; }
      if (c == '\\' && i + 1 < fmt.size()) { toks.push_back({0, string(1, fmt[i + 1])}); i += 2; continue; }
      char l = tolower(c);
      if (l == 'y' || l == 'm' || l == 'd' || l == 'h' || l == 's') { size_t j = i; while (j < fmt.size() && tolower(fmt[j]) == l) j++; toks.push_back({1, lower(fmt.substr(i, j - i))}); i = j; continue; }
      if (uf.compare(i, 5, "AM/PM") == 0) { toks.push_back({2, "AM/PM"}); i += 5; continue; }
      toks.push_back({0, string(1, c)}); i++;
    }
    bool ampm = false; for (auto& t : toks) if (t.k == 2) ampm = true;
    double days = floor(v); double frac = v - days; long long secsAll = llround(frac * 86400.0);
    if (secsAll >= 86400) { secsAll -= 86400; days += 1; }
    int y, mo, d; ymdFromSerial(days, y, mo, d);
    int hh = secsAll / 3600, mi = (secsAll / 60) % 60, ss = secsAll % 60;
    int wd = (int)((((long long)days % 7) + 7 + 6) % 7); // serial 1 = Sunday? 1899-12-31 was Sunday
    string out;
    for (size_t i = 0; i < toks.size(); i++) {
      auto& t = toks[i];
      if (t.k == 0) { out += t.s; continue; }
      if (t.k == 2) { out += hh < 12 ? "AM" : "PM"; continue; }
      char l = t.s[0]; size_t n = t.s.size(); char b[32];
      if (l == 'm') {
        bool minute = false;
        for (int j = (int)i - 1; j >= 0; j--) { if (toks[j].k == 1) { if (toks[j].s[0] == 'h') minute = true; break; } }
        for (size_t j = i + 1; j < toks.size(); j++) { if (toks[j].k == 1) { if (toks[j].s[0] == 's') minute = true; break; } }
        if (minute && n <= 2) { snprintf(b, sizeof b, n == 1 ? "%d" : "%02d", mi); out += b; continue; }
        if (n == 1) { snprintf(b, sizeof b, "%d", mo); out += b; }
        else if (n == 2) { snprintf(b, sizeof b, "%02d", mo); out += b; }
        else if (n == 3) out += string(MON[mo - 1]).substr(0, 3);
        else if (n == 5) out += string(MON[mo - 1]).substr(0, 1);
        else out += MON[mo - 1];
      } else if (l == 'd') {
        if (n == 1) { snprintf(b, sizeof b, "%d", d); out += b; }
        else if (n == 2) { snprintf(b, sizeof b, "%02d", d); out += b; }
        else if (n == 3) out += string(DAYN[wd]).substr(0, 3);
        else out += DAYN[wd];
      } else if (l == 'y') {
        if (n <= 2) { snprintf(b, sizeof b, "%02d", y % 100); out += b; } else { snprintf(b, sizeof b, "%d", y); out += b; }
      } else if (l == 'h') {
        int h = hh; if (ampm) { h = hh % 12; if (h == 0) h = 12; }
        snprintf(b, sizeof b, n == 1 ? "%d" : "%02d", h); out += b;
      } else if (l == 's') { snprintf(b, sizeof b, n == 1 ? "%d" : "%02d", ss); out += b; }
    }
    return out;
  }
  if (!hasDigit) {
    string out; for (size_t i = 0; i < fmt.size(); i++) { if (fmt[i] == '"') continue; if (fmt[i] == '\\') { i++; if (i < fmt.size()) out += fmt[i]; continue; } out += fmt[i]; }
    if (v < 0 && !negSec) out = "-" + out;
    return out;
  }
  // numeric format
  string prefix, suffix, core; int stage = 0; int pct = 0;
  for (size_t i = 0; i < fmt.size(); i++) {
    char c = fmt[i];
    if (c == '"') { size_t j = fmt.find('"', i + 1); if (j == string::npos) j = fmt.size(); string lit = fmt.substr(i + 1, j - i - 1); if (stage == 0) prefix += lit; else suffix += lit; i = j; stage = stage == 1 ? 2 : stage; continue; }
    if (c == '\\' && i + 1 < fmt.size()) { if (stage == 0) prefix += fmt[i + 1]; else suffix += fmt[i + 1]; i++; continue; }
    bool ph = (c == '0' || c == '#' || c == '?' || c == '.' || c == ',' || ((c == 'E' || c == 'e') && i + 1 < fmt.size() && (fmt[i + 1] == '+' || fmt[i + 1] == '-')));
    if (stage <= 1 && ph) {
      stage = 1;
      if (c == 'E' || c == 'e') { core += 'E'; core += fmt[i + 1]; i += 2; while (i < fmt.size() && (fmt[i] == '0' || fmt[i] == '#')) core += fmt[i++]; i--; continue; }
      core += c; continue;
    }
    if (c == '%') pct++;
    if (stage == 0) prefix += c; else { suffix += c; stage = 2; }
  }
  for (int i = 0; i < pct; i++) v *= 100;
  bool neg = v < 0; double a = fabs(v);
  size_t epos = core.find('E');
  string mant = epos == string::npos ? core : core.substr(0, epos);
  size_t dp = mant.find('.');
  string ip = dp == string::npos ? mant : mant.substr(0, dp);
  string fp = dp == string::npos ? "" : mant.substr(dp + 1);
  bool grouping = ip.find(',') != string::npos;
  // trailing commas scale
  while (!ip.empty() && ip.back() == ',') { a /= 1000; ip.pop_back(); }
  int minInt = 0; for (char c : ip) if (c == '0') minInt++;
  int fdig = 0, fmin = 0; for (char c : fp) { if (c == '0' || c == '#' || c == '?') fdig++; if (c == '0') fmin++; }
  string res;
  if (epos != string::npos) {
    string ex = core.substr(epos + 2); int edig = ex.size(); if (edig < 1) edig = 1;
    int e10 = 0; double m = a;
    if (m != 0) { e10 = (int)floor(log10(m)); m = a / pow(10.0, e10); m = roundDigits(m, fdig, 0); if (m >= 10) { m /= 10; e10++; } }
    char b[64]; snprintf(b, sizeof b, "%.*f", fdig, m); string ms = b;
    if (minInt == 0 && ms[0] == '0') ms = ms.substr(1);
    char eb[32]; snprintf(eb, sizeof eb, "%0*d", edig, abs(e10));
    res = ms + "E" + (e10 < 0 ? "-" : (core[epos + 1] == '+' ? "+" : "")) + eb;
  } else {
    double r = roundDigits(a, fdig, 0);
    char b[512]; snprintf(b, sizeof b, "%.*f", fdig, r); string s = b;
    string is = s, fs; size_t d2 = s.find('.'); if (d2 != string::npos) { is = s.substr(0, d2); fs = s.substr(d2 + 1); }
    if (is == "0" && minInt == 0) is = "";
    while ((int)is.size() < minInt) is = "0" + is;
    // optional decimals
    while ((int)fs.size() > fmin && !fs.empty() && fs.back() == '0') fs.pop_back();
    if (grouping && !is.empty()) is = groupInt(is);
    res = is; if (!fp.empty() && (dp != string::npos) && (!fs.empty() || fmin > 0 || true)) { if (!fs.empty() || fdig == 0) res += (fdig == 0 && fs.empty() ? "" : "." + fs); else res += "."; }
    if (dp != string::npos && fp.empty()) res += ".";
    bool allZero = true; for (char c : res) if (c >= '1' && c <= '9') allZero = false;
    if (allZero) neg = false;
  }
  if (neg && !negSec) return "-" + prefix + res + suffix;
  return prefix + res + suffix;
}

// ---------------- functions ----------------
static Val callFunc(Node* n);

static Val eval0(Node* n);
static Val eval(Node* n) {
  if (gCache) { Node* fn = gCache->fn; for (size_t i = 0; i < fn->a.size(); i++) if (fn->a[i] == n) return gCache->vals[i]; }
  Val v = eval0(n);
  if (n->t == N_BIN || n->t == N_FUNC) { curFmtBool = v.isb; if (v.t == V_NUM && !std::isfinite(v.n)) return mkErr(E_NUM); }
  return v;
}
static Val eval0(Node* n) {
  switch (n->t) {
    case N_NUM: return mkNum(n->n);
    case N_STR: return mkStr(n->s);
    case N_BOOL: return mkBool(n->n != 0);
    case N_ERR: return mkErr(n->op);
    case N_MISS: { Val v; v.t = V_EMPTY; return v; }
    case N_REF: return mkRef(n->sh, n->r1, n->c1, n->r2, n->c2);
    case N_NEG: case N_POS: case N_PCT: {
      Val v = eval(n->a[0]); bool fromRef = v.t == V_REF; if (fromRef) { v = deref(v); v.isb = false; } double d; int e = 0;
      if (v.t == V_ERR && v.sw) return v;
      if (!toNum(v, d, e)) return mkErr(e);
      if (n->t == N_NEG) { Val r = mkNum(-d); r.isb = v.isb; return r; }
      if (n->t == N_PCT) return mkNum(d / 100);
      return mkNum(d);
    }
    case N_BIN: {
      Val a = eval(n->a[0]); bool aRef = a.t == V_REF; if (aRef) a = deref(a);
      Val b = sc(n->a[1]);
      int op = n->op;
      if (b.t == V_ERR && b.sw) return b;
      if (a.t == V_ERR && a.sw) return a;
      if (a.t == V_ERR && !aRef) return a;
      if (b.t == V_ERR) return b;
      bool arith = !(op == '&' || op == '=' || op == 'N' || op == '<' || op == '>' || op == 'L' || op == 'G');
      if (a.t == V_ERR && arith) { double yy; int ee = 0; if (!toNum(b, yy, ee)) return mkErr(ee); return a; }
      if (a.t == V_ERR) return a;
      if (op == '&') return mkStr(toStr(a) + toStr(b));
      if (op == '=' || op == 'N' || op == '<' || op == '>' || op == 'L' || op == 'G') {
        int r = cmpVals(a, b);
        bool res;
        switch (op) { case '=': res = r == 0; break; case 'N': res = r != 0; break; case '<': res = r < 0; break; case '>': res = r > 0; break; case 'L': res = r <= 0; break; default: res = r >= 0; }
        return mkBool(res);
      }
      double x, y; int e = 0;
      if (!toNum(b, y, e)) return mkErr(e);
      if (!toNum(a, x, e)) return mkErr(e);
      double r;
      switch (op) {
        case '+': r = approxAdd(x, y); break;
        case '-': r = approxSub(x, y); break;
        case '*': r = x * y; break;
        case '/': if (y == 0) return mkErr(E_DIV0); r = x / y; break;
        case '^': { Val pv = powV(x, y); return pv; }
        default: r = 0;
      }
      if (!std::isfinite(r)) return mkErr(E_NUM);
      return mkNum(r);
    }
    case N_FUNC: return callFunc(n);
  }
  return mkErr(E_VALUE);
}

#define ARGN (int)n->a.size()
#define NUMARG(i, var) double var; { Val _v = sc(n->a[i]); int _e = 0; if (!toNum(_v, var, _e)) return mkErr(_e); }
#define STRARG(i, var) string var; { Val _v = sc(n->a[i]); if (_v.t == V_ERR) return _v; var = toStr(_v); }
static bool isMissing(Node* n, int i) { return i >= (int)n->a.size() || n->a[i]->t == N_MISS; }

static bool toBoolV(const Val& v, bool& b, int& e) {
  if (v.t == V_NUM) { b = v.n != 0; return true; }
  if (v.t == V_EMPTY) { b = false; return true; }
  if (v.t == V_ERR) { e = v.e; return false; }
  if (v.t == V_STR) { string u = upper(v.s); if (u == "TRUE") { b = true; return true; } if (u == "FALSE") { b = false; return true; } double d; if (parseNumStr(v.s, d)) { b = d != 0; return true; } }
  e = E_VALUE; return false;
}

// lookup helpers
static bool lookupEq(const Val& key, const Cell* cl, bool wild) {
  if (key.t == V_NUM) return cl->t == V_NUM && cl->n == key.n;
  if (key.t == V_STR) return cl->t == V_STR && strEqWild(key.s, cl->s, wild);
  if (key.t == V_EMPTY) return cl->t == V_EMPTY;
  return false;
}
// compare cell to key for approximate; returns 2 if type mismatch
static int lookupCmp(const Cell* cl, const Val& key) {
  if (key.t == V_NUM) { if (cl->t != V_NUM) return 2; return cl->n < key.n ? -1 : cl->n > key.n ? 1 : 0; }
  if (key.t == V_STR) { if (cl->t != V_STR) return 2; return cmpStrCI(cl->s, key.s); }
  return 2;
}

// exact-number lookup cache: key (sheet,col,r1,r2) or (row)
struct VecKey { int sh, fixed, a1, a2, vert; bool operator==(const VecKey& o) const { return sh == o.sh && fixed == o.fixed && a1 == o.a1 && a2 == o.a2 && vert == o.vert; } };
struct VecKeyH { size_t operator()(const VecKey& k) const { return ((size_t)k.sh * 1000003u) ^ ((size_t)k.fixed * 7919u) ^ ((size_t)k.a1 << 20) ^ ((size_t)k.a2 << 5) ^ k.vert; } };
static unordered_map<VecKey, unordered_map<double, int>*, VecKeyH> numIdx;
static int findExact(int sh, bool vert, int fixed, int a1, int a2, const Val& key) {
  bool wild = key.t == V_STR && hasWild(key.s);
  int len = a2 - a1 + 1;
  if (key.t == V_NUM && len >= 64) {
    VecKey k{sh, fixed, a1, a2, vert ? 1 : 0};
    auto it = numIdx.find(k);
    unordered_map<double, int>* m;
    if (it == numIdx.end()) {
      m = new unordered_map<double, int>(); m->reserve(len * 2);
      for (int i = 0; i < len; i++) { Cell* cl = vert ? cellAt(sh, a1 + i, fixed) : cellAt(sh, fixed, a1 + i); if (cl->t == V_NUM) m->emplace(cl->n, i); }
      numIdx[k] = m;
    } else m = it->second;
    auto f = m->find(key.n == 0 ? 0.0 : key.n);
    return f == m->end() ? -1 : f->second;
  }
  for (int i = 0; i < len; i++) { Cell* cl = vert ? cellAt(sh, a1 + i, fixed) : cellAt(sh, fixed, a1 + i); if (lookupEq(key, cl, wild)) return i; }
  return -1;
}
static int findApprox(int sh, bool vert, int fixed, int a1, int a2, const Val& key, bool desc) {
  int len = a2 - a1 + 1, best = -1; Cell* prevCl = nullptr; Val prevVal;

  for (int i = 0; i < len; i++) {
    Cell* cl = vert ? cellAt(sh, a1 + i, fixed) : cellAt(sh, fixed, a1 + i);
    int c = lookupCmp(cl, key);
    if (c == 2) continue;
    if (prevCl) { int pc = lookupCmp(cl, prevVal); if (pc != 2 && (desc ? pc > 0 : pc < 0)) break; }
    if (!desc) { if (c == 0) return i; if (c < 0) best = i; else break; }
    else { if (c == 0) return i; if (c > 0) best = i; else break; }
    prevCl = cl; prevVal.t = cl->t; prevVal.n = cl->n; if (cl->t == V_STR) prevVal.s = cl->s;
  }
  return best;
}

static unordered_map<string, vector<uint8_t>*> maskCache;
static const vector<uint8_t>* getMask(const Val& r, const Crit& c) {
  char kb[160];
  int L = snprintf(kb, sizeof kb, "%d,%d,%d,%d,%d|%d%d%d%d|%.17g|", r.sh, r.c1, r.c2, r.r1, r.r2, c.op, (int)c.isNum, (int)c.wild, (int)c.empty, c.n);
  string key(kb, L); key += c.s;
  auto it = maskCache.find(key);
  if (it != maskCache.end()) return it->second;
  int h = r.r2 - r.r1 + 1, w = r.c2 - r.c1 + 1;
  auto* m = new vector<uint8_t>((size_t)h * w);
  for (int dr = 0; dr < h; dr++) for (int dc = 0; dc < w; dc++) (*m)[(size_t)dr * w + dc] = critMatch(c, cellAt(r.sh, r.r1 + dr, r.c1 + dc));
  maskCache[key] = m;
  return m;
}
static Val ifsAgg(Node* n, int kind) { // kind 0 sum,1 count,2 avg ; for SUMIFS/COUNTIFS/AVERAGEIFS
  int start = kind == 1 ? 0 : 1;
  Val sumR; if (kind != 1) { sumR = eval(n->a[0]); if (sumR.t == V_ERR) return sumR; if (sumR.t != V_REF) return mkErr(E_VALUE); }
  vector<Val> ranges; vector<Crit> crits;
  for (int i = start; i + 1 < ARGN; i += 2) {
    Val r = eval(n->a[i]); if (r.t == V_ERR) return r; if (r.t != V_REF) return mkErr(E_VALUE);
    Val c = sc(n->a[i + 1]); if (c.t == V_ERR) return c;
    ranges.push_back(r); crits.push_back(makeCrit(c));
  }
  if (ranges.empty()) return mkErr(E_VALUE);
  int h = ranges[0].r2 - ranges[0].r1 + 1, w = ranges[0].c2 - ranges[0].c1 + 1;
  for (auto& r : ranges) if (r.r2 - r.r1 + 1 != h || r.c2 - r.c1 + 1 != w) return mkErr(E_VALUE);
  if (kind != 1 && (sumR.r2 - sumR.r1 + 1 != h || sumR.c2 - sumR.c1 + 1 != w)) return mkErr(E_VALUE);
  KSum sum; long cnt = 0;
  if ((size_t)h * w >= 128) {
    vector<const vector<uint8_t>*> ms; for (size_t k = 0; k < ranges.size(); k++) ms.push_back(getMask(ranges[k], crits[k]));
    vector<uint8_t> acc(*ms[0]);
    for (size_t k = 1; k < ms.size(); k++) { const uint8_t* q = ms[k]->data(); uint8_t* a = acc.data(); size_t L = acc.size(); for (size_t i = 0; i < L; i++) a[i] &= q[i]; }
    for (int dr = 0; dr < h; dr++) for (int dc = 0; dc < w; dc++) {
      if (!acc[(size_t)dr * w + dc]) continue;
      if (kind == 1) { cnt++; continue; }
      Cell* cl = cellAt(sumR.sh, sumR.r1 + dr, sumR.c1 + dc);
      if (cl->t == V_ERR) return mkErr(cl->e);
      if (cl->t == V_NUM) { sum.add(cl->n); cnt++; }
    }
    if (kind == 0) return mkNum(sum.get());
    if (kind == 1) return mkNum((double)cnt);
    if (cnt == 0) return mkErr(E_DIV0);
    return mkNum(sum.get() / cnt);
  }
  for (int dr = 0; dr < h; dr++) for (int dc = 0; dc < w; dc++) {
    bool ok = true;
    for (size_t k = 0; k < ranges.size() && ok; k++) ok = critMatch(crits[k], cellAt(ranges[k].sh, ranges[k].r1 + dr, ranges[k].c1 + dc));
    if (!ok) continue;
    if (kind == 1) { cnt++; continue; }
    Cell* cl = cellAt(sumR.sh, sumR.r1 + dr, sumR.c1 + dc);
    if (cl->t == V_ERR) return mkErr(cl->e);
    if (cl->t == V_NUM) { sum.add(cl->n); cnt++; }
  }
  if (kind == 0) return mkNum(sum.get());
  if (kind == 1) return mkNum((double)cnt);
  if (cnt == 0) return mkErr(E_DIV0);
  return mkNum(sum.get() / cnt);
}
static Val ifAgg(Node* n, int kind) { // SUMIF/COUNTIF/AVERAGEIF
  if (ARGN < 2 || ARGN > (kind == 1 ? 2 : 3)) return mkErr(E_VALUE);
  Val r = eval(n->a[0]); if (r.t == V_ERR) return r; if (r.t != V_REF) return mkErr(E_VALUE);
  Val c = sc(n->a[1]); if (c.t == V_ERR) return c;
  Crit cr = makeCrit(c);
  Val sr = r;
  if (kind != 1 && !isMissing(n, 2)) { sr = eval(n->a[2]); if (sr.t == V_ERR) return sr; if (sr.t != V_REF) return mkErr(E_VALUE); }
  KSum sum; long cnt = 0;
  int h = r.r2 - r.r1 + 1, w = r.c2 - r.c1 + 1;
  for (int dr = 0; dr < h; dr++) for (int dc = 0; dc < w; dc++) {
    if (!critMatch(cr, cellAt(r.sh, r.r1 + dr, r.c1 + dc))) continue;
    if (kind == 1) { cnt++; continue; }
    Cell* cl = cellAt(sr.sh, sr.r1 + dr, sr.c1 + dc);
    if (cl->t == V_ERR) return mkErr(cl->e);
    if (cl->t == V_NUM) { sum.add(cl->n); cnt++; }
  }
  if (kind == 0) return mkNum(sum.get());
  if (kind == 1) return mkNum((double)cnt);
  if (cnt == 0) return mkErr(E_DIV0);
  return mkNum(sum.get() / cnt);
}

static Val callFunc0(Node* n);
static unordered_set<string> exemptF = {"IF","IFERROR","IFNA","IFS","SWITCH","CHOOSE","ISERROR","ISNA","ISBLANK","ISNUMBER","ISTEXT","ISLOGICAL","COUNT","COUNTA","XLOOKUP","N","T","TRUE","FALSE","PI"};
static unordered_set<string> noDerefF = {"SUMIF","SUMIFS","COUNTIF","COUNTIFS","AVERAGEIF","AVERAGEIFS","VLOOKUP","HLOOKUP","MATCH","INDEX","COUNTBLANK","SUMPRODUCT"};
static Val callFunc(Node* n) {
  if (n->op == 0) n->op = exemptF.count(n->s) ? 1 : noDerefF.count(n->s) ? 3 : 2;
  if (n->op == 1 || n->a.empty()) {
    ArgCache* sv = gCache; gCache = nullptr; Val r = callFunc0(n); gCache = sv;
    if (r.t == V_ERR) { int f = n->fid; if (f == FID_IFERROR || f == FID_IFNA || f == FID_IFS || f == FID_SWITCH || f == FID_XLOOKUP) r.sw = true; }
    return r;
  }
  size_t na = n->a.size();
  Val* vals = (Val*)alloca(sizeof(Val) * na);
  ArgCache* sv = gCache; gCache = nullptr;
  size_t built = 0; Val r; int lastStrong = -1; bool anyErr = false; int strongErr = 0;
  for (size_t k = 0; k < na; k++) {
    new (&vals[k]) Val(eval(n->a[k])); built++;
    Node* ak = n->a[k];
    if (ak->t == N_FUNC) { int f = ak->fid; if (f == FID_IFERROR || f == FID_IFNA || f == FID_IFS || f == FID_SWITCH || f == FID_XLOOKUP) { lastStrong = (int)k; if (vals[k].t == V_ERR) strongErr = vals[k].e; } }
    if (vals[k].t == V_ERR) anyErr = true;
  }
  if (anyErr) {
    int err = strongErr;
    if (!err) for (size_t k = lastStrong + 1; k < na; k++) if (vals[k].t == V_ERR) { err = vals[k].e; break; }
    if (!err) for (int k = lastStrong; k >= 0; k--) if (vals[k].t == V_ERR) { err = vals[k].e; break; }
    r = mkErr(err); for (size_t j = 0; j < built; j++) vals[j].~Val(); gCache = sv; return r;
  }
  ArgCache ac{n, vals}; gCache = &ac;
  r = callFunc0(n);
  gCache = sv;
  for (size_t j = 0; j < built; j++) vals[j].~Val();
  return r;
}
static Val callFunc0(Node* n) {
  const int fid = n->fid;
  int na = ARGN;
  // ---- logical / control ----
  if (fid == FID_IF) {
    if (na < 1) return mkErr(E_VALUE);
    Val c = sc(n->a[0]); bool b; int e = 0;
    if (!toBoolV(c, b, e)) return mkErr(e);
    curFmtBool = false;
    if (b) { if (na < 2) return mkBool(true); if (n->a[1]->t == N_MISS) return mkNum(0); Val v = eval(n->a[1]); return v; }
    if (na < 3) return mkBool(false);
    if (n->a[2]->t == N_MISS) return mkNum(0);
    return eval(n->a[2]);
  }
  if (fid == FID_IFERROR) { Val raw = eval(n->a[0]); Val v = deref(raw); if (v.t == V_ERR) return na > 1 ? eval(n->a[1]) : mkNum(0); return raw; }
  if (fid == FID_IFNA) { Val raw = eval(n->a[0]); Val v = deref(raw); if (v.t == V_ERR && v.e == E_NA) return na > 1 ? eval(n->a[1]) : mkNum(0); return raw; }
  if (fid == FID_IFS) {
    for (int i = 0; i + 1 < na; i += 2) { Val c = sc(n->a[i]); bool b; int e = 0; if (!toBoolV(c, b, e)) return mkErr(e); if (b) return eval(n->a[i + 1]); }
    return mkErr(E_NA);
  }
  if (fid == FID_SWITCH) {
    Val v = sc(n->a[0]); if (v.t == V_ERR) { Val r = mkErr(v.e); r.sw = true; return r; }
    int i = 1;
    for (; i + 1 < na; i += 2) { Val c = sc(n->a[i]); if (c.t == V_ERR) return c; if (cmpVals(v, c) == 0 && !((v.t == V_STR) != (c.t == V_STR))) return eval(n->a[i + 1]); }
    if (i < na) return eval(n->a[i]);
    return mkErr(E_NA);
  }
  if (fid == FID_CHOOSE) {
    NUMARG(0, d); int k = (int)d;
    if (k < 1 || k >= na) return mkErr(E_VALUE);
    return eval(n->a[k]);
  }
  if (fid == FID_AND || fid == FID_OR || fid == FID_XOR) {
    int cnt = 0; bool acc = fid == FID_AND; int x = 0; int err = 0;
    for (Node* a : n->a) {
      if (a->t == N_MISS) continue;
      bool ok = iterArg(a, [&](VT t, double v, int e, const string& s, bool rng, Cell*) {
        if (t == V_ERR) { err = e; return false; }
        if (t == V_NUM) { cnt++; bool b = v != 0; if (fid == FID_AND) acc = acc && b; else if (fid == FID_OR) acc = acc || b; else x ^= b; return true; }
        if (!rng && t == V_STR) { err = E_VALUE; return false; }
        if (!rng && t == V_EMPTY) { cnt++; if (fid == FID_AND) acc = false; return true; }
        return true;
      }, err);
      if (!ok) return mkErr(err);
    }
    if (cnt == 0) return mkErr(E_VALUE);
    if (fid == FID_XOR) return mkBool(x);
    return mkBool(acc);
  }
  if (fid == FID_NOT) { Val v = sc(n->a[0]); bool b; int e = 0; if (!toBoolV(v, b, e)) return mkErr(e); return mkBool(!b); }
  if (fid == FID_TRUE) return mkBool(true);
  if (fid == FID_FALSE) return mkBool(false);
  // ---- info ----
  if (fid == FID_ISBLANK) {
    Val v = eval(n->a[0]);
    if (v.t == V_REF) { if (v.r1 != v.r2 || v.c1 != v.c2) { Val d = deref(v); return mkBool(false); } Cell* cl = getCell(v.sh, v.r1, v.c1); return mkBool(cl->t == V_EMPTY && !cl->f); }
    return mkBool(false);
  }
  if (fid == FID_ISERROR) { Val v = sc(n->a[0]); return mkBool(v.t == V_ERR); }
  if (fid == FID_ISNA) { Val v = sc(n->a[0]); return mkBool(v.t == V_ERR && v.e == E_NA); }
  if (fid == FID_ISNUMBER) { Val v = sc(n->a[0]); return mkBool(v.t == V_NUM); }
  if (fid == FID_ISTEXT) { Val v = sc(n->a[0]); return mkBool(v.t == V_STR); }
  if (fid == FID_ISLOGICAL) { Node* a0 = n->a[0]; bool lit = a0->t == N_STR || a0->t == N_NUM; bool before = curFmtBool; Val v = sc(a0); if (lit) return mkBool(before); return mkBool(v.t == V_NUM && v.isb); }
  if (fid == FID_N) { Val v = sc(n->a[0]); if (v.t == V_ERR) return v; if (v.t == V_NUM) return mkNum(v.n); return mkNum(0); }
  if (fid == FID_T) { Val v = sc(n->a[0]); if (v.t == V_ERR) return v; if (v.t == V_STR) return v; return mkStr(""); }
  // ---- math ----
  if (fid == FID_ABS) { NUMARG(0, x); return mkNum(fabs(x)); }
  if (fid == FID_INT) { NUMARG(0, x); return mkNum(floor(x)); }
  if (fid == FID_SIGN) { NUMARG(0, x); return mkNum(x > 0 ? 1 : x < 0 ? -1 : 0); }
  if (fid == FID_SQRT) { NUMARG(0, x); if (x < 0) return mkErr(E_VALUE); return mkNum(sqrt(x)); }
  if (fid == FID_EXP) { NUMARG(0, x); double r = exp(x); if (!std::isfinite(r)) return mkErr(E_NUM); return mkNum(r); }
  if (fid == FID_LN) { NUMARG(0, x); if (x <= 0) return mkErr(E_VALUE); return mkNum(log(x)); }
  if (fid == FID_LOG10) { NUMARG(0, x); if (x <= 0) return mkErr(E_VALUE); return mkNum(log10(x)); }
  if (fid == FID_LOG) {
    NUMARG(0, x); double b = 10; if (!isMissing(n, 1)) { NUMARG(1, bb); b = bb; }
    if (x <= 0 || b <= 0) return mkErr(E_VALUE);
    if (b == 1) return mkErr(E_VALUE);
    return mkNum(log(x) / log(b));
  }
  if (fid == FID_PI) return mkNum(M_PI);
  if (fid == FID_POWER) {
    NUMARG(0, x); NUMARG(1, y);
    return powV(x, y);
  }
  if (fid == FID_MOD) {
    NUMARG(1, y);
    if (y == 0) return mkErr(E_DIV0);
    NUMARG(0, x);
    double q = x / y; double fl = floor(q); { double frac = q - fl; if (frac > 1 - 1e-9 || frac < 1e-9) fl = floor(roundSig(q)); }
    double r = approxSub(x, y * fl);
    if (r != 0 && fabs(r - y) < fabs(y) * 1e-15) r = 0;
    if (y > 0 && r < 0) r = 0; if (y < 0 && r > 0) r = 0;
    return mkNum(r);
  }
  if (fid == FID_ROUND || fid == FID_ROUNDDOWN || fid == FID_ROUNDUP) {
    NUMARG(0, x); double d = 0; if (!isMissing(n, 1)) { NUMARG(1, dd); d = dd; }
    int di = (int)(d < 0 ? -floor(-d) : floor(d));
    return mkNum(roundDigits(x, di, fid == FID_ROUND ? 0 : fid == FID_ROUNDDOWN ? 1 : 2));
  }
  if (fid == FID_TRUNC) {
    NUMARG(0, x); double d = 0; if (!isMissing(n, 1)) { NUMARG(1, dd); d = dd; }
    return mkNum(roundDigits(x, (int)d, 1));
  }
  if (fid == FID_SUM || fid == FID_PRODUCT || fid == FID_SUMSQ || fid == FID_AVERAGE || fid == FID_MAX || fid == FID_MIN || fid == FID_MEDIAN || fid == FID_STDEV || fid == FID_VAR || fid == FID_COUNT) {
    if (fid == FID_COUNT) {
      long cnt = 0; int err = 0;
      for (Node* a : n->a) {
        if (a->t == N_MISS) continue;
        iterArg(a, [&](VT t, double, int, const string& s, bool rng, Cell*) {
          if (t == V_NUM) cnt++;
          else if (!rng && t == V_STR) { double d; if (parseNumStr(s, d)) cnt++; }
          return true;
        }, err);
      }
      return mkNum((double)cnt);
    }
    vector<double> v; int err = 0;
    if (!gatherNums(n, v, err)) return mkErr(err);
    if (fid == FID_SUM) { KSum s; for (double x : v) s.add(x); return mkNum(s.get()); }
    if (fid == FID_SUMSQ) { KSum s; for (double x : v) s.add(x * x); return mkNum(s.get()); }
    if (fid == FID_PRODUCT) { if (v.empty()) return mkNum(0); double s = 1; for (double x : v) s *= x; return mkNum(s); }
    if (fid == FID_AVERAGE) { if (v.empty()) return mkErr(E_DIV0); KSum s; for (double x : v) s.add(x); return mkNum(s.get() / v.size()); }
    if (fid == FID_MAX || fid == FID_MIN) {
      if (v.empty()) return mkNum(0);
      Val r = mkNum(fid == FID_MAX ? *max_element(v.begin(), v.end()) : *min_element(v.begin(), v.end()));
      {
        bool have = false, bb = false; double best = 0; bool isMax = fid == FID_MAX; int e2 = 0;
        for (Node* a : n->a) {
          if (a->t == N_MISS) continue;
          iterArg(a, [&](VT t, double x, int, const string&, bool, Cell* cl) {
            if (t != V_NUM) return true;
            bool ib = cl ? cl->isb : false;
            if (!have) { have = true; best = x; bb = ib; }
            return true; }, e2);
        }
        r.isb = bb;

      }
      return r;
    }
    if (fid == FID_MEDIAN) {
      if (v.empty()) return mkErr(E_VALUE);
      sort(v.begin(), v.end()); size_t m = v.size();
      return mkNum(m % 2 ? v[m / 2] : (v[m / 2 - 1] + v[m / 2]) / 2);
    }
    // STDEV / VAR
    if (v.size() < 2) return mkErr(E_DIV0);
    KSum ms; for (double x : v) ms.add(x); double mean = ms.get() / v.size();
    double ss = 0; for (double x : v) ss += (x - mean) * (x - mean);
    double var = ss / (v.size() - 1);
    return mkNum(fid == FID_VAR ? var : sqrt(var));
  }
  if (fid == FID_LARGE || fid == FID_SMALL) {
    vector<double> v; int err = 0;
    { int e2 = 0; bool ok = iterArg(n->a[0], [&](VT t, double x, int e, const string& s, bool rng, Cell*) {
        if (t == V_ERR) { err = e; return false; }
        if (t == V_NUM) v.push_back(x);
        else if (!rng && t == V_STR) { double d; if (parseNumStr(s, d)) v.push_back(d); else { err = E_VALUE; return false; } }
        return true; }, e2);
      if (!ok) return mkErr(err); }
    NUMARG(1, kd);
    long k = (long)ceil(kd - 1e-12);
    if (k < 1 || k > (long)v.size()) return mkErr(E_VALUE);
    if (fid == FID_LARGE) { nth_element(v.begin(), v.begin() + (k - 1), v.end(), greater<double>()); }
    else nth_element(v.begin(), v.begin() + (k - 1), v.end());
    return mkNum(v[k - 1]);
  }
  if (fid == FID_COUNTA) {
    long cnt = 0; int err = 0;
    for (Node* a : n->a) {
      if (a->t == N_MISS) continue;
      iterArg(a, [&](VT t, double, int, const string&, bool rng, Cell* cl) { if (t != V_EMPTY || (cl && cl->f)) cnt++; else if (!rng) cnt++; return true; }, err);
    }
    return mkNum((double)cnt);
  }
  if (fid == FID_COUNTBLANK) {
    Val r = eval(n->a[0]); if (r.t != V_REF) return mkErr(E_VALUE);
    long cnt = 0;
    for (int i = r.r1; i <= r.r2; i++) for (int j = r.c1; j <= r.c2; j++) { Cell* cl = cellAt(r.sh, i, j); if (cl->t == V_EMPTY || (cl->t == V_STR && cl->s.empty())) cnt++; }
    return mkNum((double)cnt);
  }
  if (fid == FID_SUMPRODUCT) {
    vector<Val> rs;
    for (Node* a : n->a) { Val v = eval(a); if (v.t == V_ERR) return v; rs.push_back(v); }
    if (rs.empty()) return mkErr(E_VALUE);
    for (auto& v : rs) if (v.t != V_REF) { // scalar
      if (rs.size() == 1) { double d; int e = 0; if (!toNum(v, d, e)) return mkErr(e); return mkNum(d); }
    }
    int h = -1, w = -1;
    for (auto& v : rs) { int hh = v.t == V_REF ? v.r2 - v.r1 + 1 : 1, ww = v.t == V_REF ? v.c2 - v.c1 + 1 : 1; if (h < 0) { h = hh; w = ww; } else if (hh != h || ww != w) return mkErr(E_VALUE); }
    KSum s;
    for (int i = 0; i < h; i++) for (int j = 0; j < w; j++) {
      double p = 1;
      for (auto& v : rs) {
        if (v.t == V_REF) { Cell* cl = cellAt(v.sh, v.r1 + i, v.c1 + j); if (cl->t == V_ERR) return mkErr(cl->e); p *= cl->t == V_NUM ? cl->n : 0; }
        else { p *= v.t == V_NUM ? v.n : 0; }
      }
      s.add(p);
    }
    return mkNum(s.get());
  }
  if (fid == FID_SUMIF) return ifAgg(n, 0);
  if (fid == FID_COUNTIF) return ifAgg(n, 1);
  if (fid == FID_AVERAGEIF) return ifAgg(n, 2);
  if (fid == FID_SUMIFS) return ifsAgg(n, 0);
  if (fid == FID_COUNTIFS) return ifsAgg(n, 1);
  if (fid == FID_AVERAGEIFS) return ifsAgg(n, 2);
  // ---- text ----
  if (fid == FID_LEN) { STRARG(0, s); return mkNum((double)s.size()); }
  if (fid == FID_LOWER) { STRARG(0, s); return mkStr(lower(s)); }
  if (fid == FID_UPPER) { STRARG(0, s); return mkStr(upper(s)); }
  if (fid == FID_PROPER) {
    STRARG(0, s); bool prev = false;
    for (auto& c : s) { if (isalpha((unsigned char)c)) { c = prev ? tolower(c) : toupper(c); prev = true; } else { prev = false; } }
    return mkStr(s);
  }
  if (fid == FID_TRIM) {
    STRARG(0, s); string r; bool sp = false;
    for (char c : s) { if (c == ' ') { sp = true; } else { if (sp && !r.empty()) r += ' '; sp = false; r += c; } }
    return mkStr(r);
  }
  if (fid == FID_LEFT || fid == FID_RIGHT) {
    STRARG(0, s); double k = 1; if (!isMissing(n, 1)) { NUMARG(1, kk); k = kk; }
    if (k < 0) return mkErr(E_VALUE);
    size_t kn = (size_t)min<double>(k, (double)s.size());
    return mkStr(fid == FID_LEFT ? s.substr(0, kn) : s.substr(s.size() - kn));
  }
  if (fid == FID_MID) {
    STRARG(0, s); NUMARG(1, st); NUMARG(2, ln);
    if (st < 1 || ln < 0) return mkErr(E_VALUE);
    size_t b = (size_t)st - 1; if (b >= s.size()) return mkStr("");
    return mkStr(s.substr(b, (size_t)min<double>(ln, (double)(s.size() - b))));
  }
  if (fid == FID_REPT) {
    STRARG(0, s); NUMARG(1, k); if (k < 0) return mkErr(E_VALUE);
    string r; long kk = (long)k; for (long i = 0; i < kk; i++) r += s; return mkStr(r);
  }
  if (fid == FID_EXACT) { STRARG(0, a); STRARG(1, b); return mkBool(a == b); }
  if (fid == FID_FIND || fid == FID_SEARCH) {
    STRARG(0, needle); STRARG(1, hay); double st = 1; if (!isMissing(n, 2)) { NUMARG(2, s2); st = s2; }
    if (st < 1 || st > hay.size() + (needle.empty() ? 1 : 0)) { if (!(needle.empty() && st >= 1 && st <= hay.size() + 1)) return mkErr(E_VALUE); }
    size_t b = (size_t)st - 1;
    if (needle.empty()) return mkErr(E_VALUE);
    if (fid == FID_FIND) { size_t pos = hay.find(needle, b); if (pos == string::npos) return mkErr(E_VALUE); return mkNum((double)pos + 1); }
    bool wild = hasWild(needle);
    if (!wild) { string h = lower(hay), nd = lower(needle); size_t pos = h.find(nd, b); if (pos == string::npos) return mkErr(E_VALUE); return mkNum((double)pos + 1); }
    for (size_t i = b; i < hay.size(); i++) for (size_t j = i; j <= hay.size(); j++) if (wildMatch(needle.data(), needle.data() + needle.size(), hay.data() + i, hay.data() + j)) return mkNum((double)i + 1);
    return mkErr(E_VALUE);
  }
  if (fid == FID_SUBSTITUTE) {
    STRARG(0, s); STRARG(1, from); STRARG(2, to);
    long inst = 0; if (!isMissing(n, 3)) { NUMARG(3, k); if (k < 1) return mkErr(E_VALUE); inst = (long)k; }
    if (from.empty()) return mkStr(s);
    string r; size_t pos = 0; long cnt = 0;
    for (;;) {
      size_t q = s.find(from, pos);
      if (q == string::npos) { r += s.substr(pos); break; }
      cnt++;
      if (inst == 0 || cnt == inst) { r += s.substr(pos, q - pos) + to; } else { r += s.substr(pos, q - pos + from.size()); }
      pos = q + from.size();
    }
    return mkStr(r);
  }
  if (fid == FID_REPLACE) {
    STRARG(0, s); NUMARG(1, st); NUMARG(2, ln); STRARG(3, nw);
    if (st < 1 || ln < 0) return mkErr(E_VALUE);
    size_t b = min((size_t)st - 1, s.size()); size_t l = min((size_t)ln, s.size() - b);
    return mkStr(s.substr(0, b) + nw + s.substr(b + l));
  }
  if (fid == FID_CONCATENATE) { string r; for (int i = 0; i < na; i++) { STRARG(i, s); r += s; } return mkStr(r); }
  if (fid == FID_CONCAT || fid == FID_TEXTJOIN) {
    string r; int start = 0; string delim; bool ignoreEmpty = false;
    if (fid == FID_TEXTJOIN) { STRARG(0, d); delim = d; Val b = sc(n->a[1]); bool bb; int e = 0; if (!toBoolV(b, bb, e)) return mkErr(e); ignoreEmpty = bb; start = 2; }
    bool first = true; int err = 0;
    for (int i = start; i < na; i++) {
      bool ok = iterArg(n->a[i], [&](VT t, double v, int e, const string& s, bool, Cell*) {
        if (t == V_ERR) { err = e; return false; }
        string x = t == V_NUM ? fmtNum(v) : t == V_STR ? s : string();
        if (ignoreEmpty && x.empty()) return true;
        if (!first) r += delim; first = false; r += x; return true;
      }, err);
      if (!ok) return mkErr(err);
    }
    return mkStr(r);
  }
  if (fid == FID_VALUE) {
    Val v = eval(n->a[0]); if (v.t == V_REF) { v = deref(v); v.isb = false; } if (v.t == V_ERR) return v;
    if (v.t == V_NUM) return v;
    if (v.t == V_EMPTY) return mkNum(0);
    double d; if (parseNumStr(v.s, d)) return mkNum(d);
    { string u = upper(v.s); if (u == "TRUE") return mkNum(1); if (u == "FALSE") return mkNum(0); }
    return mkErr(E_VALUE);
  }
  if (fid == FID_TEXT) {
    Val v = sc(n->a[0]); if (v.t == V_ERR) return v;
    STRARG(1, fmt);
    double d;
    if (v.t == V_STR) { if (!parseNumStr(v.s, d)) return v; }
    else d = v.t == V_NUM ? v.n : 0;
    return mkStr(formatText(d, fmt));
  }
  // ---- dates ----
  if (fid == FID_DATE) {
    NUMARG(0, y); NUMARG(1, m); NUMARG(2, d);
    long long yy = (long long)floor(y), mm = (long long)floor(m), dd = (long long)floor(d);
    if (yy < 0 || yy > 9999) return mkErr(E_VALUE);
    if (yy < 1900) yy += 1900;
    double s = serialFromYMD(yy, mm, dd);
    if (s < 0) return mkErr(E_VALUE);
    return mkNum(s);
  }
  if (fid == FID_YEAR || fid == FID_MONTH || fid == FID_DAY) {
    NUMARG(0, x);
    int y, m, d; ymdFromSerial(x, y, m, d);
    return mkNum(fid == FID_YEAR ? y : fid == FID_MONTH ? m : d);
  }
  if (fid == FID_WEEKDAY) {
    NUMARG(0, x); int type = 1; if (!isMissing(n, 1)) { NUMARG(1, t); type = (int)t; }
    long long dn = (long long)floor(x);
    int wd = (int)(((dn % 7) + 7 + 6) % 7); // 0=Sunday
    switch (type) {
      case 1: return mkNum(wd + 1);
      case 2: return mkNum((wd + 6) % 7 + 1);
      case 3: return mkNum((wd + 6) % 7);
      case 11: case 12: case 13: case 14: case 15: case 16: case 17: { int startD = (type - 10) % 7; /* 11 = monday start */ int st = type == 17 ? 0 : type - 10; return mkNum(((wd - st) % 7 + 7) % 7 + 1); }
      default: return mkErr(E_VALUE);
    }
  }
  if (fid == FID_DAYS) { NUMARG(0, a); NUMARG(1, b); return mkNum(a - b); }
  if (fid == FID_EDATE || fid == FID_EOMONTH) {
    NUMARG(0, s); NUMARG(1, m);
    int y, mo, d; ymdFromSerial(trunc(s), y, mo, d);
    long long mm = (long long)(m < 0 ? -floor(-m) : floor(m));
    long long tot = (long long)y * 12 + (mo - 1) + mm;
    long long ny = tot >= 0 ? tot / 12 : -((-tot + 11) / 12); int nm = (int)(tot - ny * 12) + 1;
    int dim = daysInMonth((int)ny, nm);
    int nd = fid == FID_EOMONTH ? dim : min(d, dim);
    double r = serialFromYMD(ny, nm, nd);
    return mkNum(r);
  }
  if (fid == FID_DATEDIF) {
    NUMARG(0, a); NUMARG(1, b); STRARG(2, u);
    a = floor(a); b = floor(b);
    if (a > b) return mkErr(E_VALUE);
    int y1, m1, d1, y2, m2, d2; ymdFromSerial(a, y1, m1, d1); ymdFromSerial(b, y2, m2, d2);
    string uu = upper(u);
    if (uu == "D") return mkNum(b - a);
    int months = (y2 - y1) * 12 + (m2 - m1); if (d2 < d1) months--;
    if (uu == "M") return mkNum(months);
    if (uu == "Y") return mkNum(months / 12);
    if (uu == "YM") return mkNum(months % 12);
    if (uu == "MD") {
      if (d2 >= d1) return mkNum(d2 - d1);
      // days from d1 in previous month to d2
      int pm = m2 - 1, py = y2; if (pm < 1) { pm = 12; py--; }
      double prev = serialFromYMD(py, pm, d1);
      return mkNum(b - prev);
    }
    if (uu == "YD") {
      int yy = y2; double st = serialFromYMD(yy, m1, d1);
      if (st > b) { yy--; st = serialFromYMD(yy, m1, d1); }
      return mkNum(b - st);
    }
    return mkErr(E_VALUE);
  }
  // ---- lookup ----
  if (fid == FID_VLOOKUP || fid == FID_HLOOKUP) {
    if (na < 3) return mkErr(E_VALUE);
    Val key = sc(n->a[0]); if (key.t == V_ERR) return key;
    if (key.t == V_EMPTY) { key.t = V_NUM; key.n = 0; }
    Val r = eval(n->a[1]); if (r.t == V_ERR) return r; if (r.t != V_REF) return mkErr(E_VALUE);
    NUMARG(2, ci); int idx = (int)ci;
    bool approx = true; if (!isMissing(n, 3)) { Val b = sc(n->a[3]); bool bb; int e = 0; if (!toBoolV(b, bb, e)) return mkErr(e); approx = bb; }
    bool vert = fid == FID_VLOOKUP;
    int width = vert ? r.c2 - r.c1 + 1 : r.r2 - r.r1 + 1;
    if (idx < 1) return mkErr(E_VALUE);
    if (idx > width) return mkErr(E_VALUE);
    int pos;
    if (vert) pos = approx ? findApprox(r.sh, true, r.c1, r.r1, r.r2, key, false) : findExact(r.sh, true, r.c1, r.r1, r.r2, key);
    else pos = approx ? findApprox(r.sh, false, r.r1, r.c1, r.c2, key, false) : findExact(r.sh, false, r.r1, r.c1, r.c2, key);
    if (pos < 0) return mkErr(E_NA);
    Val res = vert ? cellVal(r.sh, r.r1 + pos, r.c1 + idx - 1) : cellVal(r.sh, r.r1 + idx - 1, r.c1 + pos);
    res.isb = false;
    return res;
  }
  if (fid == FID_MATCH) {
    Val key = sc(n->a[0]); if (key.t == V_ERR) return key;
    Val r = eval(n->a[1]); if (r.t == V_ERR) return r; if (r.t != V_REF) return mkErr(E_NA);
    int type = 1; if (!isMissing(n, 2)) { NUMARG(2, t); type = t > 0 ? 1 : t < 0 ? -1 : 0; }
    bool vert = r.c1 == r.c2;
    if (!vert && r.r1 != r.r2) return mkErr(E_VALUE);
    int fixed = vert ? r.c1 : r.r1, a1 = vert ? r.r1 : r.c1, a2 = vert ? r.r2 : r.c2;
    int pos = type == 0 ? findExact(r.sh, vert, fixed, a1, a2, key) : findApprox(r.sh, vert, fixed, a1, a2, key, type < 0);
    if (pos < 0) return mkErr(E_NA);
    return mkNum(pos + 1);
  }
  if (fid == FID_XLOOKUP) {
    if (na < 3) return mkErr(E_VALUE);
    Val key = sc(n->a[0]); if (key.t == V_ERR) return mkErr(E_VALUE);
    Val lr = eval(n->a[1]); if (lr.t == V_ERR) return lr; if (lr.t != V_REF) return mkErr(E_VALUE);
    Val rr = eval(n->a[2]); if (rr.t == V_ERR) return rr; if (rr.t != V_REF) return mkErr(E_VALUE);
    int mode = 0, smode = 1;
    if (!isMissing(n, 4)) { NUMARG(4, m); mode = (int)m; }
    if (!isMissing(n, 5)) { NUMARG(5, s); smode = (int)s; }
    bool vert = lr.c1 == lr.c2;
    int fixed = vert ? lr.c1 : lr.r1, a1 = vert ? lr.r1 : lr.c1, a2 = vert ? lr.r2 : lr.c2;
    int len = a2 - a1 + 1, pos = -1;
    if (mode == 0 && smode == 1) pos = findExact(lr.sh, vert, fixed, a1, a2, key);
    else {
      bool wild = mode == 2 && key.t == V_STR;
      int bestI = -1; Val bestV;
      for (int k = 0; k < len; k++) {
        int i = smode < 0 ? len - 1 - k : k;
        Cell* cl = vert ? cellAt(lr.sh, a1 + i, fixed) : cellAt(lr.sh, fixed, a1 + i);
        if (mode == 0 || mode == 2) { if (lookupEq(key, cl, wild)) { pos = i; break; } continue; }
        int c = lookupCmp(cl, key);
        if (c == 2) continue;
        if (c == 0) { pos = i; break; }
        if (mode == -1 && c < 0) { if (bestI < 0 || (key.t == V_NUM ? cl->n > getCell(lr.sh, vert ? a1 + bestI : fixed, vert ? fixed : a1 + bestI)->n : cmpStrCI(cl->s, getCell(lr.sh, vert ? a1 + bestI : fixed, vert ? fixed : a1 + bestI)->s) > 0)) bestI = i; }
        if (mode == 1 && c > 0) { if (bestI < 0 || (key.t == V_NUM ? cl->n < getCell(lr.sh, vert ? a1 + bestI : fixed, vert ? fixed : a1 + bestI)->n : cmpStrCI(cl->s, getCell(lr.sh, vert ? a1 + bestI : fixed, vert ? fixed : a1 + bestI)->s) < 0)) bestI = i; }
      }
      if (pos < 0) pos = bestI;
    }
    if (pos < 0) { if (!isMissing(n, 3)) return sc(n->a[3]); return mkErr(E_NA); }
    if (vert) return mkRef(rr.sh, rr.r1 + pos, rr.c1, rr.r1 + pos, rr.c2);
    return mkRef(rr.sh, rr.r1, rr.c1 + pos, rr.r2, rr.c1 + pos);
  }
  if (fid == FID_INDEX) {
    Val r = eval(n->a[0]); if (r.t == V_ERR) return r; if (r.t != V_REF) return mkErr(E_VALUE);
    double rd = 0, cd = 0;
    if (!isMissing(n, 1)) { NUMARG(1, x); rd = x; }
    if (!isMissing(n, 2)) { NUMARG(2, x); cd = x; }
    int ri = (int)rd, ci = (int)cd;
    int h = r.r2 - r.r1 + 1, w = r.c2 - r.c1 + 1;
    if (na == 2 && h == 1 && w > 1) { ci = ri; ri = 0; }

    if (ri < 0 || ci < 0 || ri > h || ci > w) return mkErr(E_REF);
    if (ri == 0 && ci == 0) return r;
    if (ri == 0) return mkRef(r.sh, r.r1, r.c1 + ci - 1, r.r2, r.c1 + ci - 1);
    if (ci == 0) { if (w == 1) ci = 1; else return mkRef(r.sh, r.r1 + ri - 1, r.c1, r.r1 + ri - 1, r.c2); }
    return mkRef(r.sh, r.r1 + ri - 1, r.c1 + ci - 1, r.r1 + ri - 1, r.c1 + ci - 1);
  }
  return mkErr(E_NAME);
}

// ---------------- I/O ----------------
static string readZip(zip_t* z, const string& name) {
  zip_stat_t st; if (zip_stat(z, name.c_str(), 0, &st) != 0) return "";
  zip_file_t* f = zip_fopen(z, name.c_str(), 0); if (!f) return "";
  string buf; buf.resize(st.size);
  zip_int64_t got = 0; while (got < (zip_int64_t)st.size) { zip_int64_t k = zip_fread(f, &buf[got], st.size - got); if (k <= 0) break; got += k; }
  zip_fclose(f); return buf;
}
static int colFromRef(const char* s, int& row) {
  int col = 0; while (*s && isalpha((unsigned char)*s)) { col = col * 26 + (toupper(*s) - 'A' + 1); s++; }
  row = atoi(s); return col;
}
static void jsonStr(string& out, const string& s) {
  out += '"';
  for (unsigned char c : s) {
    switch (c) { case '"': out += "\\\""; break; case '\\': out += "\\\\"; break; case '\n': out += "\\n"; break; case '\r': out += "\\r"; break; case '\t': out += "\\t"; break;
      default: if (c < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); out += b; } else out += (char)c; }
  }
  out += '"';
}
static string colName(int c) { string s; c++; while (c > 0) { int m = (c - 1) % 26; s = (char)('A' + m) + s; c = (c - 1) / 26; } return s; }

struct RawCell { int r, c; VT t; bool isb; double n; string s; int e; string f; Node* node = nullptr; };
static Node* parseFormula(const string& f, int si) {
  Parser P(f, si); Node* nd = P.expr(); P.ws();
  if (P.p < f.size()) { nd = newNode(); nd->t = N_ERR; nd->op = E_NAME; }
  return nd;
}
static int gScanSheet = 0;
static void appendUtf8(string& o, unsigned cp) {
  if (cp < 0x80) o += (char)cp;
  else if (cp < 0x800) { o += (char)(0xC0 | (cp >> 6)); o += (char)(0x80 | (cp & 0x3F)); }
  else if (cp < 0x10000) { o += (char)(0xE0 | (cp >> 12)); o += (char)(0x80 | ((cp >> 6) & 0x3F)); o += (char)(0x80 | (cp & 0x3F)); }
  else { o += (char)(0xF0 | (cp >> 18)); o += (char)(0x80 | ((cp >> 12) & 0x3F)); o += (char)(0x80 | ((cp >> 6) & 0x3F)); o += (char)(0x80 | (cp & 0x3F)); }
}
static void xmlText(const char* a, const char* b, string& o) {
  o.clear();
  const char* amp = (const char*)memchr(a, '&', b - a);
  if (!amp) { o.assign(a, b); return; }
  o.reserve(b - a);
  while (a < b) {
    if (*a != '&') { o += *a++; continue; }
    const char* sc = (const char*)memchr(a, ';', b - a);
    if (!sc) { o += *a++; continue; }
    string ent(a + 1, sc);
    if (ent == "lt") o += '<'; else if (ent == "gt") o += '>'; else if (ent == "amp") o += '&'; else if (ent == "quot") o += '"'; else if (ent == "apos") o += '\'';
    else if (!ent.empty() && ent[0] == '#') { unsigned cp = ent.size() > 1 && (ent[1] == 'x' || ent[1] == 'X') ? strtoul(ent.c_str() + 2, nullptr, 16) : strtoul(ent.c_str() + 1, nullptr, 10); appendUtf8(o, cp); }
    else { o.append(a, sc + 1); }
    a = sc + 1;
  }
}
// fast scanner for worksheet xml; returns false if structure unexpected
static bool scanRange(const char* p, const char* sdEnd, vector<RawCell>& out, const vector<string>& sst, int& maxR, int& maxC);
static bool scanSheet(const string& x, vector<RawCell>& out, const vector<string>& sst, int& maxR, int& maxC) {
  const char* p = x.c_str(); const char* end = p + x.size();
  const char* sd = strstr(p, "<sheetData"); if (!sd) return true;
  p = sd;
  const char* sdEnd = strstr(p, "</sheetData>"); if (!sdEnd) sdEnd = end;
  size_t len = sdEnd - p;
  int NTH = len > (1 << 20) ? 4 : 1;
  if (NTH == 1) return scanRange(p, sdEnd, out, sst, maxR, maxC);
  vector<const char*> bounds; bounds.push_back(p);
  for (int i = 1; i < NTH; i++) {
    const char* t = p + len * i / NTH; if (t < bounds.back()) t = bounds.back();
    const char* f = strstr(t, "<row"); if (!f || f > sdEnd) f = sdEnd;
    bounds.push_back(f);
  }
  bounds.push_back(sdEnd);
  vector<vector<RawCell>> parts(NTH); vector<int> mr(NTH, 0), mc(NTH, 0); vector<char> ok(NTH, 1);
  vector<thread> th;
  for (int i = 0; i < NTH; i++) th.emplace_back([&, i]() { ok[i] = scanRange(bounds[i], bounds[i + 1], parts[i], sst, mr[i], mc[i]); });
  for (auto& t : th) t.join();
  size_t tot = 0; for (int i = 0; i < NTH; i++) { if (!ok[i]) return false; tot += parts[i].size(); maxR = max(maxR, mr[i]); maxC = max(maxC, mc[i]); }
  out.reserve(tot);
  for (int i = 0; i < NTH; i++) for (auto& c : parts[i]) out.push_back(std::move(c));
  return true;
}
static bool scanRange(const char* p, const char* sdEnd, vector<RawCell>& out, const vector<string>& sst, int& maxR, int& maxC) {
  string tmp;
  while (p < sdEnd) {
    const char* c = (const char*)memchr(p, '<', sdEnd - p); if (!c) break;
    if (c[1] != 'c' || !(c[2] == ' ' || c[2] == '>' || c[2] == '/')) { p = c + 1; continue; }
    // attributes
    const char* q = c + 2; const char* tagEnd = (const char*)memchr(q, '>', sdEnd - q); if (!tagEnd) return false;
    bool selfClose = tagEnd[-1] == '/';
    const char* rA = nullptr; const char* rB = nullptr; const char* tA = nullptr; const char* tB = nullptr;
    while (q < tagEnd) {
      while (q < tagEnd && (*q == ' ' || *q == '\t' || *q == '\n' || *q == '\r')) q++;
      const char* nm = q; while (q < tagEnd && *q != '=' && *q != ' ' && *q != '/') q++;
      size_t nl = q - nm;
      if (q >= tagEnd || *q != '=') { q++; continue; }
      q++; char quo = *q; if (quo != '"' && quo != '\'') return false; q++;
      const char* va = q; while (q < tagEnd && *q != quo) q++; const char* vb = q; q++;
      if (nl == 1 && nm[0] == 'r') { rA = va; rB = vb; } else if (nl == 1 && nm[0] == 't') { tA = va; tB = vb; }
    }
    p = tagEnd + 1;
    if (!rA) return false;
    RawCell cell; int rr = 0; int col = 0; const char* z = rA;
    while (z < rB && isalpha((unsigned char)*z)) { col = col * 26 + (toupper(*z) - 'A' + 1); z++; }
    while (z < rB && isdigit((unsigned char)*z)) { rr = rr * 10 + (*z - '0'); z++; }
    cell.r = rr - 1; cell.c = col - 1; cell.t = V_EMPTY; cell.isb = false; cell.n = 0; cell.e = 0;
    if (selfClose) continue;
    const char* ce = strstr(p, "</c>"); if (!ce || ce > sdEnd) return false;
    string t = tA ? string(tA, tB) : string();
    bool hasF = false, hasV = false; string vtxt, istr;
    const char* w = p;
    while (w < ce) {
      const char* lt = (const char*)memchr(w, '<', ce - w); if (!lt) break;
      char k = lt[1];
      const char* ge = (const char*)memchr(lt, '>', ce - lt); if (!ge) return false;
      bool sc2 = ge[-1] == '/';
      if ((k == 'f' || k == 'v') && (lt[2] == '>' || lt[2] == ' ' || lt[2] == '/')) {
        if (sc2) { if (k == 'f') hasF = true; w = ge + 1; continue; }
        const char* close = strstr(ge + 1, k == 'f' ? "</f>" : "</v>"); if (!close) return false;
        if (k == 'f') { hasF = true; xmlText(ge + 1, close, cell.f); } else { hasV = true; xmlText(ge + 1, close, vtxt); }
        w = close + 4; continue;
      }
      if (k == 't' && (lt[2] == '>' || lt[2] == ' ')) {
        if (sc2) { w = ge + 1; continue; }
        const char* close = strstr(ge + 1, "</t>"); if (!close) return false;
        xmlText(ge + 1, close, tmp); istr += tmp; w = close + 4; continue;
      }
      w = ge + 1;
    }
    p = ce + 4;
    if (hasF) { if (cell.f.empty()) cell.f = " "; cell.node = parseFormula(cell.f, gScanSheet); }
    else {
      if (t == "inlineStr") { cell.t = V_STR; cell.s = istr; }
      else if (t == "s") { int k = atoi(vtxt.c_str()); cell.t = V_STR; cell.s = k >= 0 && k < (int)sst.size() ? sst[k] : ""; }
      else if (t == "str") { cell.t = V_STR; cell.s = vtxt; }
      else if (t == "b") { if (hasV) { cell.t = V_NUM; cell.isb = true; cell.n = atoi(vtxt.c_str()) != 0; } }
      else if (t == "e") { cell.t = V_ERR; cell.e = E_NA; static const char* es[] = {"#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"}; for (int k = 0; k < 7; k++) if (vtxt == es[k]) cell.e = k + 1; }
      else { if (hasV && !vtxt.empty()) { cell.t = V_NUM; cell.n = strtod(vtxt.c_str(), nullptr); } }
      if (cell.t == V_EMPTY) continue;
    }
    maxR = max(maxR, cell.r + 1); maxC = max(maxC, cell.c + 1);
    out.push_back(std::move(cell));
  }
  return true;
}

static double tnow() { return chrono::duration<double>(chrono::steady_clock::now().time_since_epoch()).count(); }
static bool TT = getenv("CALC_TIME") != nullptr; static double T0 = tnow();
#define TMARK(x) do { if (TT) fprintf(stderr, "%s %.4f\n", x, tnow() - T0); } while (0)
static int realMain(int argc, char** argv) {
  string in, out;
  for (int i = 1; i < argc; i++) { string a = argv[i]; if (a == "--in" && i + 1 < argc) in = argv[++i]; else if (a == "--out" && i + 1 < argc) out = argv[++i]; }
  int zerr; zip_t* z = zip_open(in.c_str(), ZIP_RDONLY, &zerr);
  if (!z) { fprintf(stderr, "cannot open %s\n", in.c_str()); return 1; }
  // shared strings
  vector<string> sst;
  {
    string x = readZip(z, "xl/sharedStrings.xml");
    if (!x.empty()) {
      pugi::xml_document d; d.load_buffer(x.data(), x.size());
      for (auto si : d.child("sst").children("si")) {
        string s; if (si.child("t")) s = si.child("t").text().get();
        else for (auto r : si.children("r")) s += r.child("t").text().get();
        sst.push_back(s);
      }
    }
  }
  // workbook + rels
  vector<pair<string, string>> sheetList;
  {
    string rels = readZip(z, "xl/_rels/workbook.xml.rels");
    unordered_map<string, string> rid;
    pugi::xml_document rd; rd.load_buffer(rels.data(), rels.size());
    for (auto r : rd.child("Relationships").children("Relationship")) {
      string t = r.attribute("Target").value();
      if (!t.empty() && t[0] == '/') t = t.substr(1); else t = "xl/" + t;
      rid[r.attribute("Id").value()] = t;
    }
    string wb = readZip(z, "xl/workbook.xml");
    pugi::xml_document wd; wd.load_buffer(wb.data(), wb.size());
    for (auto s : wd.child("workbook").child("sheets").children("sheet")) {
      string id = s.attribute("r:id").value();
      sheetList.push_back({s.attribute("name").value(), rid[id]});
    }
  }
  sheets.resize(sheetList.size());
  for (size_t i = 0; i < sheetList.size(); i++) { sheets[i].name = sheetList[i].first; sheetIdx[sheetList[i].first] = (int)i; }
  TMARK("wb");
  vector<vector<RawCell>> raw(sheets.size());
  for (size_t si = 0; si < sheetList.size(); si++) {
    string x = readZip(z, sheetList[si].second);
    TMARK("unzip");
    {
      int mR = 0, mC = 0; vector<RawCell> tmpv;
      gScanSheet = (int)si;
      if (!getenv("CALC_PUGI") && scanSheet(x, tmpv, sst, mR, mC)) {
        raw[si] = std::move(tmpv);
        TMARK("xml");
        sheets[si].nr = mR; sheets[si].nc = mC;
        sheets[si].cells.resize((size_t)mR * mC);
        continue;
      }
    }
    pugi::xml_document d; d.load_buffer_inplace(&x[0], x.size(), pugi::parse_default & ~pugi::parse_wconv_attribute);
    int maxR = 0, maxC = 0;
    auto& rc = raw[si];
    for (auto row : d.child("worksheet").child("sheetData").children("row")) {
      for (auto c : row.children("c")) {
        RawCell cell; int rr; int cc = colFromRef(c.attribute("r").value(), rr);
        cell.r = rr - 1; cell.c = cc - 1; cell.t = V_EMPTY; cell.isb = false; cell.n = 0; cell.e = 0;
        string t = c.attribute("t").value();
        auto fn = c.child("f");
        if (fn) { cell.f = fn.text().get(); }
        auto vn = c.child("v");
        if (!fn) {
          if (t == "inlineStr") { auto is = c.child("is"); string s; if (is.child("t")) s = is.child("t").text().get(); else for (auto r : is.children("r")) s += r.child("t").text().get(); cell.t = V_STR; cell.s = s; }
          else if (t == "s") { int k = atoi(vn.text().get()); cell.t = V_STR; cell.s = k >= 0 && k < (int)sst.size() ? sst[k] : ""; }
          else if (t == "str") { cell.t = V_STR; cell.s = vn.text().get(); }
          else if (t == "b") { if (vn) { cell.t = V_NUM; cell.isb = true; cell.n = atoi(vn.text().get()) != 0; } }
          else if (t == "e") { cell.t = V_ERR; string ev = vn.text().get(); cell.e = E_NA; static const char* es[] = {"#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"}; for (int k = 0; k < 7; k++) if (ev == es[k]) cell.e = k + 1; }
          else { if (vn && *vn.text().get()) { cell.t = V_NUM; cell.n = strtod(vn.text().get(), nullptr); } }
          if (cell.t == V_EMPTY) continue;
        }
        maxR = max(maxR, cell.r + 1); maxC = max(maxC, cell.c + 1);
        rc.push_back(std::move(cell));
      }
    }
    TMARK("xml");
    sheets[si].nr = maxR; sheets[si].nc = maxC;
    sheets[si].cells.resize((size_t)maxR * maxC);
  }
  zip_close(z);
  // place cells & parse formulas
  for (size_t si = 0; si < raw.size(); si++) {
    auto& R = raw[si];
    size_t N = R.size();
    int NT_ = N > 20000 ? 4 : 1;
    auto work = [&](int tid) {
      size_t a = N * tid / NT_, b = N * (tid + 1) / NT_;
      for (size_t i = a; i < b; i++) {
        auto& rc = R[i];
        Cell& cl = sheets[si].cells[(size_t)rc.r * sheets[si].nc + rc.c];
        cl.t = rc.t; cl.isb = rc.isb; cl.n = rc.n; cl.s = std::move(rc.s); cl.e = rc.e;
        if (rc.node) { cl.f = rc.node; cl.t = V_EMPTY; }
        else if (!rc.f.empty()) { cl.f = parseFormula(rc.f, (int)si); cl.t = V_EMPTY; }
      }
    };
    if (NT_ == 1) work(0);
    else { vector<thread> th; for (int t = 1; t < NT_; t++) th.emplace_back(work, t); work(0); for (auto& t : th) t.join(); }
  }
  TMARK("parse");
  // evaluate all
  for (size_t si = 0; si < sheets.size(); si++) {
    Sheet& S = sheets[si];
    for (int r = 0; r < S.nr; r++) for (int c = 0; c < S.nc; c++) {
      Cell* cl = &S.cells[(size_t)r * S.nc + c];
      if (cl->f && cl->st != 2) computeCell(cl, (int)si, r, c);
    }
  }
  TMARK("compute");
  string js = "{"; js.reserve(1 << 20);
  bool firstSheet = true;
  for (size_t si = 0; si < sheets.size(); si++) {
    Sheet& S = sheets[si];
    int JT = (size_t)S.nr * S.nc > 50000 ? 4 : 1;
    vector<string> parts(JT);
    auto jw = [&](int t) {
    string& part = parts[t]; part.reserve(1 << 20);
    int ra = (int)((long long)S.nr * t / JT), rb2 = (int)((long long)S.nr * (t + 1) / JT);
    for (int r = ra; r < rb2; r++) for (int c = 0; c < S.nc; c++) {
      Cell* cl = &S.cells[(size_t)r * S.nc + c];
      if (!cl->f) continue;
      part += ',';
      part += '"'; part += colName(c); { char rb[16]; auto rr = std::to_chars(rb, rb + 16, r + 1); part.append(rb, rr.ptr); } part += "\":{\"t\":";
      switch (cl->t) {
        case V_NUM: if (cl->isb && (cl->n == 0 || cl->n == 1)) { part += "\"b\",\"v\":"; part += cl->n != 0 ? "true" : "false"; } else { char b[40]; auto rr = std::to_chars(b, b + 40, cl->n); *rr.ptr = 0; part += "\"n\",\"v\":"; part += b; } break;
        case V_STR: part += "\"s\",\"v\":"; jsonStr(part, cl->s); break;
        case V_ERR: part += "\"e\",\"v\":\""; part += errStr(cl->e); part += '"'; break;
        default: part += "\"n\",\"v\":0"; break;
      }
      part += '}';
    }
    };
    if (JT == 1) jw(0); else { vector<thread> th; for (int t = 1; t < JT; t++) th.emplace_back(jw, t); jw(0); for (auto& t : th) t.join(); }
    bool any = false; for (auto& p : parts) if (!p.empty()) any = true;
    if (!any) continue;
    if (!firstSheet) js += ','; firstSheet = false;
    jsonStr(js, S.name); js += ":{";
    bool firstP = true;
    for (auto& p : parts) { if (p.empty()) continue; if (firstP) { js.append(p, 1, string::npos); firstP = false; } else js += p; }
    js += '}';
  }
  js += "}";
  TMARK("eval");
  FILE* fo = fopen(out.c_str(), "wb"); if (!fo) return 1;
  fwrite(js.data(), 1, js.size(), fo); fclose(fo);
  return 0;
}

struct Args { int argc; char** argv; int ret; };
static void* thr(void* p) { Args* a = (Args*)p; a->ret = realMain(a->argc, a->argv); return nullptr; }
int main(int argc, char** argv) {
  Args a{argc, argv, 0};
  size_t sizes[] = {(size_t)2048 << 20, (size_t)512 << 20, (size_t)128 << 20, (size_t)32 << 20};
  bool started = false; pthread_t t;
  for (size_t sz : sizes) {
    pthread_attr_t at; pthread_attr_init(&at); pthread_attr_setstacksize(&at, sz);
    if (pthread_create(&t, &at, thr, &a) == 0) { started = true; break; }
  }
  if (!started) return realMain(argc, argv);
  pthread_join(t, nullptr);
  fflush(stdout);
  _exit(a.ret);
}
