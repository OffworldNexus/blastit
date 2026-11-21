/*
 * Blastfile highlighting for the documentation site (MB.32.1).
 *
 * Why hand-written: Pygments has no `Blastfile` lexer, the site is built
 * offline with `uvx zensical build --strict`, and a plugin, a build step or a
 * bundled grammar would each add a dependency the docs gate cannot afford. So
 * this is one classic script with no modules, no network access and no dynamic
 * code: it reads the text Zensical already rendered and wraps tokens in the
 * span classes the theme's own Pygments rules style.
 *
 * It never rebuilds `code.innerHTML`. It splits Text nodes, so the
 * `<span id="__span-…">` line wrappers, the `<a id="__codelineno-…">` anchors
 * and `code.textContent` (what the copy button reads) are left byte-for-byte
 * as the build wrote them.
 *
 * The same file is the test surface (`globalThis.BlastfileLexer`): `tokenize`
 * and `classify` are pure functions of the fence text, and `highlightCode` is
 * the only part that touches the document.
 */
(function (global) {
  "use strict";

  var MARK = "data-blastfile-highlighted";

  // --- The Blastfile vocabulary, one set per kind -------------------------
  function lookup(list) {
    var map = {};
    for (var i = 0; i < list.length; i++) {
      map[list[i]] = true;
    }
    return map;
  }

  var SECTION = lookup([
    "app", "entrypoint", "postgres", "trait", "view", "static"
  ]);
  var DIRECTIVE = lookup([
    "body", "root", "immutable", "preload", "host", "port", "user",
    "password", "database", "sslmode", "fallback", "unvalidated"
  ]);
  var GRANT = lookup([
    "use", "select", "insert", "update", "delete", "query", "transactions",
    "functions", "unrestricted"
  ]);
  var RUNTIME = lookup(["python", "php"]);
  var METHOD = lookup(["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]);

  // One SQL keyword set. It is the set the reference page and the Zed grammar
  // agree on; the asset test pins it, so a fork shows up as a failing name.
  var SQL_KEYWORDS = lookup([
    "SELECT", "FROM", "WHERE", "JOIN", "INSERT", "INTO", "UPDATE", "SET",
    "DELETE", "VALUES", "IN", "AND", "OR", "NOT", "NULL", "ORDER", "BY",
    "GROUP", "HAVING", "LIMIT", "OFFSET", "RETURNING", "ON", "CONFLICT", "DO",
    "NOTHING", "AS", "DISTINCT", "EXISTS", "CASE", "WHEN", "THEN", "ELSE",
    "END", "WITH", "UNION", "ALL", "IS", "LIKE", "BETWEEN", "LEFT", "RIGHT",
    "INNER", "OUTER", "COUNT", "BEGIN", "COMMIT", "ROLLBACK"
  ]);

  // --- The classes, each one styled by Zensical's own Pygments CSS --------
  var C = {
    keyword: "k",
    declaration: "kd",
    constant: "kc",
    type: "kt",
    name: "nf",
    variable: "nv",
    raw: "s1",
    string: "s2",
    escape: "se",
    comment: "c1",
    punctuation: "p",
    number: "mi",
    operator: "o"
  };
  var TOKEN_CLASSES = [
    C.keyword, C.declaration, C.constant, C.type, C.name, C.variable, C.raw,
    C.string, C.escape, C.comment, C.punctuation, C.number, C.operator
  ];

  var SQL_OPERATORS = [
    "->>", "->", "::", "<>", "<=", ">=", "||", "=", "<", ">", "+", "-", "*",
    "/", "%"
  ];

  // --- Classification: is this fence a Blastfile at all? ------------------
  // Only a first significant line that opens with a Blastfile statement word
  // is a Blastfile. A grammar arrow, a `::=` production or a
  // `<file>:<line>:<col>:` diagnostic is refused outright, so the error
  // sentence and the EBNF stay plain even after a stray `query` line.
  function classify(text) {
    var lines = String(text).split("\n");
    for (var i = 0; i < lines.length; i++) {
      var line = lines[i].replace(/^\s+|\s+$/g, "");
      if (!line || line.charAt(0) === "#") {
        continue;
      }
      if (/^\S+:[0-9]+:[0-9]+:/.test(line) || line.indexOf("\u2192") >= 0 ||
          line.indexOf("::=") >= 0) {
        return false;
      }
      var word = /^[A-Za-z_][A-Za-z0-9_-]*/.exec(line);
      if (!word) {
        return false;
      }
      return SECTION[word[0]] === true || DIRECTIVE[word[0]] === true ||
        GRANT[word[0]] === true;
    }
    return false;
  }

  function emit(out, cls, text) {
    if (text) {
      out.push({ text: text, cls: cls || null });
    }
  }

  function isSpace(ch) {
    return ch === " " || ch === "\t" || ch === "\n" || ch === "\r" ||
      ch === "\f" || ch === "\v";
  }

  function isWordStart(ch) {
    return (ch >= "A" && ch <= "Z") || (ch >= "a" && ch <= "z") || ch === "_";
  }

  function isWordChar(ch) {
    return isWordStart(ch) || (ch >= "0" && ch <= "9");
  }

  function isDigit(ch) {
    return ch >= "0" && ch <= "9";
  }

  function operatorAt(text, i) {
    for (var k = 0; k < SQL_OPERATORS.length; k++) {
      var op = SQL_OPERATORS[k];
      if (text.substr(i, op.length) === op) {
        return op;
      }
    }
    return "";
  }

  // --- The SQL inside a `query` string ------------------------------------
  // Runs on the string's interior only, so a `--` comment can never reach the
  // closing quote: the interior ends where the string does. `...` is one
  // token, never three operators.
  function sqlTokens(interior) {
    var out = [];
    var i = 0;
    var n = interior.length;
    while (i < n) {
      var ch = interior.charAt(i);
      if (ch === "-" && interior.charAt(i + 1) === "-") {
        var stop = interior.indexOf("\n", i);
        if (stop < 0) {
          stop = n;
        }
        emit(out, C.comment, interior.slice(i, stop));
        i = stop;
        continue;
      }
      if (ch === "$") {
        if (interior.charAt(i + 1) === "?") {
          emit(out, C.escape, "$?");
          i += 2;
          continue;
        }
        var digits = /^[0-9]+/.exec(interior.slice(i + 1));
        if (digits) {
          emit(out, C.variable, "$" + digits[0]);
          i += 1 + digits[0].length;
          continue;
        }
      }
      if (ch === "'" || ch === '"') {
        // A quoted identifier or an inline literal is opaque: `"Order"` is not
        // the keyword ORDER, and `'a''b'` is one run.
        var closeAt = i + 1;
        while (closeAt < n) {
          if (interior.charAt(closeAt) === ch) {
            if (interior.charAt(closeAt + 1) === ch) {
              closeAt += 2;
              continue;
            }
            break;
          }
          closeAt += 1;
        }
        var quotedEnd = Math.min(closeAt + 1, n);
        emit(out, null, interior.slice(i, quotedEnd));
        i = quotedEnd;
        continue;
      }
      if (ch === "." && interior.substr(i, 3) === "..." &&
          interior.charAt(i - 1) !== "." && interior.charAt(i + 3) !== ".") {
        emit(out, C.variable, "...");
        i += 3;
        continue;
      }
      if (isDigit(ch) && !isWordChar(interior.charAt(i - 1))) {
        var number = /^[0-9]+(\.[0-9]+)?/.exec(interior.slice(i));
        emit(out, C.number, number[0]);
        i += number[0].length;
        continue;
      }
      if (isWordStart(ch)) {
        var word = /^[A-Za-z_][A-Za-z0-9_]*/.exec(interior.slice(i))[0];
        emit(out, SQL_KEYWORDS[word.toUpperCase()] ? C.keyword : null, word);
        i += word.length;
        continue;
      }
      var op = operatorAt(interior, i);
      if (op) {
        emit(out, C.operator, op);
        i += op.length;
        continue;
      }
      emit(out, null, ch);
      i += 1;
    }
    return out;
  }

  // The interior of a `query` string, with `"…"`'s four escapes kept as their
  // own .se tokens so an escaped quote never ends the SQL run early.
  function queryTokens(quote, interior) {
    var out = [];
    var run = "";
    var i = 0;
    while (i < interior.length) {
      if (quote === '"' && interior.charAt(i) === "\\") {
        if (run) {
          pushRun(out, run);
          run = "";
        }
        emit(out, C.escape, interior.substr(i, 2));
        i += 2;
        continue;
      }
      run += interior.charAt(i);
      i += 1;
    }
    if (run) {
      pushRun(out, run);
    }
    return out;
  }

  function pushRun(out, run) {
    var tokens = sqlTokens(run);
    for (var t = 0; t < tokens.length; t++) {
      emit(out, tokens[t].cls, tokens[t].text);
    }
  }

  // --- A Blastfile string, one pass ---------------------------------------
  // `'…'` has no escapes; `"…"` escapes `\"`, `\\`, `\n`, `\t` as .se; a
  // `"…"` that starts with `/` or `*` is a route pattern, whose
  // `{name:type}` carries a variable name and a type.
  function stringTokens(text, i, query) {
    var quote = text.charAt(i);
    var quoteClass = quote === "'" ? C.raw : C.string;
    var j = i + 1;
    var closed = false;
    while (j < text.length) {
      var ch = text.charAt(j);
      if (ch === "\\" && quote === '"') {
        j += 2;
        continue;
      }
      if (ch === quote) {
        closed = true;
        break;
      }
      j += 1;
    }
    var interiorEnd = closed ? j : text.length;
    var interior = text.slice(i + 1, interiorEnd);
    var out = [];
    emit(out, quoteClass, quote);

    if (query) {
      var tokens = queryTokens(quote, interior);
      for (var t = 0; t < tokens.length; t++) {
        emit(out, tokens[t].cls, tokens[t].text);
      }
    } else if (quote === '"' &&
               (interior.charAt(0) === "/" || interior.charAt(0) === "*")) {
      var route = 0;
      var plain = "";
      while (route < interior.length) {
        if (interior.charAt(route) === "{") {
          var close = interior.indexOf("}", route);
          if (close < 0) {
            plain += interior.slice(route);
            break;
          }
          emit(out, quoteClass, plain);
          plain = "";
          emit(out, C.punctuation, "{");
          var spec = interior.slice(route + 1, close);
          var colon = spec.indexOf(":");
          if (colon < 0) {
            emit(out, C.variable, spec);
          } else {
            emit(out, C.variable, spec.slice(0, colon));
            emit(out, C.punctuation, ":");
            emit(out, C.type, spec.slice(colon + 1));
          }
          emit(out, C.punctuation, "}");
          route = close + 1;
          continue;
        }
        plain += interior.charAt(route);
        route += 1;
      }
      emit(out, quoteClass, plain);
    } else {
      var start = 0;
      var index = 0;
      while (index < interior.length) {
        if (quote === '"' && interior.charAt(index) === "\\") {
          emit(out, quoteClass, interior.slice(start, index));
          emit(out, C.escape, interior.slice(index, index + 2));
          index += 2;
          start = index;
          continue;
        }
        index += 1;
      }
      emit(out, quoteClass, interior.slice(start));
    }

    emit(out, quoteClass, closed ? quote : "");
    return { tokens: out, end: closed ? j + 1 : text.length };
  }

  // --- The Blastfile lexer -------------------------------------------------
  // One pass. `query` only becomes a SQL context when it is the directive
  // (not a name) and the next significant character is a quote: a `query`
  // inside a comment, or a view named `query`, colours nothing as SQL.
  function tokenize(text) {
    var source = String(text);
    var out = [];
    var i = 0;
    var n = source.length;
    var expectName = false;
    var nameList = false;
    var nameClass = C.type;
    var query = false;

    while (i < n) {
      var ch = source.charAt(i);

      if (ch === "#") {
        var stop = source.indexOf("\n", i);
        if (stop < 0) {
          stop = n;
        }
        emit(out, C.comment, source.slice(i, stop));
        i = stop;
        query = false;
        expectName = false;
        nameList = false;
        continue;
      }

      if (ch === "'" || ch === '"') {
        var scanned = stringTokens(source, i, query);
        for (var t = 0; t < scanned.tokens.length; t++) {
          emit(out, scanned.tokens[t].cls, scanned.tokens[t].text);
        }
        i = scanned.end;
        query = false;
        expectName = false;
        nameList = false;
        continue;
      }

      if (isSpace(ch)) {
        var space = i;
        while (i < n && isSpace(source.charAt(i))) {
          i += 1;
        }
        emit(out, null, source.slice(space, i));
        continue;
      }

      if (ch === "$") {
        var digits = /^[A-Za-z_][A-Za-z0-9_]*/.exec(source.slice(i + 1));
        if (digits) {
          emit(out, C.variable, "$" + digits[0]);
          i += 1 + digits[0].length;
        } else if (source.charAt(i + 1) === "?") {
          emit(out, C.escape, "$?");
          i += 2;
        } else {
          emit(out, null, ch);
          i += 1;
        }
        query = false;
        expectName = false;
        nameList = false;
        continue;
      }

      if (ch === "{" || ch === "}" || ch === "(" || ch === ")" ||
          ch === "," || ch === ";") {
        emit(out, C.punctuation, ch);
        i += 1;
        if (ch === ",") {
          if (nameList) {
            expectName = true;
          }
        } else if (ch === "{" || ch === "}" || ch === ";" || ch === ")") {
          expectName = false;
          nameList = false;
        }
        query = false;
        continue;
      }

      if (isWordStart(ch)) {
        var word = /^[A-Za-z_][A-Za-z0-9_-]*/.exec(source.slice(i))[0];
        query = false;
        if (expectName) {
          emit(out, nameClass, word);
          expectName = false;
        } else if (SECTION[word] === true || GRANT[word] === true) {
          emit(out, C.keyword, word);
          if (word === "view") {
            expectName = true;
            nameClass = C.name;
          } else if (word === "trait" || word === "postgres") {
            expectName = true;
            nameClass = C.type;
          } else if (word === "use") {
            expectName = true;
            nameList = true;
            nameClass = C.type;
          } else if (word === "query") {
            query = true;
          }
        } else if (DIRECTIVE[word] === true) {
          emit(out, C.declaration, word);
        } else if (RUNTIME[word] === true) {
          emit(out, C.type, word);
        } else if (METHOD[word] === true) {
          emit(out, C.constant, word);
        } else {
          emit(out, null, word);
        }
        i += word.length;
        continue;
      }

      emit(out, null, ch);
      i += 1;
    }
    return out;
  }

  // --- The DOM walk --------------------------------------------------------
  // Splits Text nodes only. Every element, every id and every attribute stays
  // where the build put it, and the concatenated text is unchanged.
  function textNodes(root) {
    var found = [];
    var stack = [root];
    while (stack.length) {
      var children = stack.pop().childNodes || [];
      for (var i = children.length - 1; i >= 0; i--) {
        var child = children[i];
        if (child.nodeType === 3) {
          found.push(child);
        } else if (child.nodeType === 1) {
          stack.push(child);
        }
      }
    }
    return found;
  }

  function wrap(cls, value) {
    var span = document.createElement("span");
    span.className = cls;
    span.appendChild(document.createTextNode(value));
    return span;
  }

  function highlightCode(code) {
    if (code.hasAttribute(MARK)) {
      return;
    }
    var text = code.textContent || "";
    if (!classify(text)) {
      return;
    }
    var segments = tokenize(text);
    var nodes = textNodes(code);
    var offset = 0;
    var segment = 0;
    var at = 0;

    for (var n = 0; n < nodes.length; n++) {
      var node = nodes[n];
      var data = node.nodeValue || "";
      var end = offset + data.length;
      var pieces = [];
      var cursor = offset;
      while (cursor < end) {
        while (segment < segments.length &&
               at + segments[segment].text.length <= cursor) {
          at += segments[segment].text.length;
          segment += 1;
        }
        if (segment >= segments.length) {
          pieces.push([data.slice(cursor - offset), null]);
          break;
        }
        var current = segments[segment];
        var stop = Math.min(end, at + current.text.length);
        pieces.push([data.slice(cursor - offset, stop - offset), current.cls]);
        cursor = stop;
      }
      var parent = node.parentNode;
      var changed = pieces.length !== 1 || pieces[0][1] !== null;
      if (changed && parent) {
        for (var p = 0; p < pieces.length; p++) {
          if (!pieces[p][0]) {
            continue;
          }
          var next = pieces[p][1]
            ? wrap(pieces[p][1], pieces[p][0])
            : document.createTextNode(pieces[p][0]);
          parent.insertBefore(next, node);
        }
        parent.removeChild(node);
      }
      offset = end;
    }
    code.setAttribute(MARK, "");
  }

  function highlight() {
    var blocks = document.querySelectorAll(".highlight.language-text");
    for (var i = 0; i < blocks.length; i++) {
      var code = blocks[i].querySelector("pre > code") ||
        blocks[i].querySelector("code");
      if (code) {
        highlightCode(code);
      }
    }
  }

  global.BlastfileLexer = {
    tokenize: tokenize,
    classify: classify,
    highlightCode: highlightCode,
    highlight: highlight,
    TOKEN_CLASSES: TOKEN_CLASSES,
    MARK: MARK
  };

  // `navigation.instant` swaps the document without reloading the page, so the
  // highlighter subscribes to Material's observable when it exists and falls
  // back to the first load otherwise.
  if (typeof document$ !== "undefined" && document$.subscribe) {
    document$.subscribe(highlight);
  } else if (typeof document !== "undefined") {
    document.addEventListener("DOMContentLoaded", highlight);
  }
})(typeof globalThis !== "undefined" ? globalThis : this);
