// 500.com 历史数据源（备选 / 补齐 2013 年以前）
//
// 接口: GET https://datachart.500.com/ssq/history/newinc/history.php?start=YY001&end=YY999
// 说明: 返回 HTML 表格。表格列：
//       期号 | 红球 x6 | 蓝球 | 快乐星期天 | 奖池 | 一等奖注数 | 一等奖金额 |
//       二等奖注数 | 二等奖金额 | 总投注额 | 开奖日期
//
// 宿主提供: http.get(url) -> string, log(message)
// 返回值: JSON 字符串数组
//
// 性能注意（重要）：内嵌 JS 引擎处理大字符串是超线性的
// （实测对 1.4MB 整页做 split/indexOf/正则都在 40~120 秒）。
// 所以这里按年份分段请求，每段约 60KB，并用 indexOf 游标扫描解析，
// 循环内不使用正则。整轮回填可控制在数秒内。

(function () {
  // 由 Rust 侧替换：只回填到哪一年（缺主数据源时会更晚）
  var endYear = __END_YEAR__;
  var startYear = 2003;
  var base = "https://datachart.500.com/ssq/history/newinc/history.php";

  function stripNbsp(text) {
    var idx = text.indexOf("&nbsp;");
    while (idx >= 0) {
      text = text.substring(0, idx) + text.substring(idx + 6);
      idx = text.indexOf("&nbsp;");
    }
    return text;
  }

  function trim(text) {
    var start = 0;
    var end = text.length;
    while (start < end && text.charCodeAt(start) <= 32) {
      start = start + 1;
    }
    while (end > start && text.charCodeAt(end - 1) <= 32) {
      end = end - 1;
    }
    return text.substring(start, end);
  }

  function isDigits(text) {
    if (text.length === 0) {
      return false;
    }
    for (var i = 0; i < text.length; i++) {
      var c = text.charCodeAt(i);
      if (c < 48 || c > 57) {
        return false;
      }
    }
    return true;
  }

  function isDigitsOrCommas(text) {
    if (text.length === 0) {
      return false;
    }
    for (var i = 0; i < text.length; i++) {
      var c = text.charCodeAt(i);
      if (c !== 44 && (c < 48 || c > 57)) {
        return false;
      }
    }
    return true;
  }

  function toInt(text) {
    var digits = "";
    for (var i = 0; i < text.length; i++) {
      var c = text.charAt(i);
      if (c >= "0" && c <= "9") {
        digits = digits + c;
      }
    }
    return digits.length === 0 ? 0 : parseInt(digits, 10);
  }

  function cellText(cell) {
    var gt = cell.indexOf(">");
    if (gt < 0) {
      return "";
    }
    var body = cell.substring(gt + 1);
    var close = body.indexOf("</td>");
    if (close >= 0) {
      body = body.substring(0, close);
    }
    return trim(stripNbsp(body));
  }

  // 解析一段 HTML（单年份，约 60KB）
  function parsePage(html, collected) {
    var needle = '<tr class="t_tr1">';
    var pos = html.indexOf(needle);

    while (pos >= 0) {
      var start = pos + needle.length;
      var rowEnd = html.indexOf("</tr>", start);
      if (rowEnd < 0) {
        break;
      }
      var row = html.substring(start, rowEnd);
      pos = html.indexOf(needle, rowEnd);

      var cells = row.split("<td");
      var reds = [];
      var blue = 0;
      var code = "";
      var date = "";
      var pool = null;
      var sales = null;

      for (var j = 1; j < cells.length; j++) {
        var cell = cells[j];
        var gt = cell.indexOf(">");
        if (gt < 0) {
          continue;
        }
        var attrs = cell.substring(0, gt);
        var text = cellText(cell);

        if (attrs.indexOf("t_cfont2") >= 0) {
          if (isDigits(text)) {
            reds.push(toInt(text));
          }
        } else if (attrs.indexOf("t_cfont4") >= 0) {
          if (blue === 0 && isDigits(text)) {
            blue = toInt(text);
          }
        } else if (code === "" && text.length === 5 && isDigits(text)) {
          code = text;
        }

        if (date === "" && text.length === 10 && text.charAt(4) === "-" && text.charAt(7) === "-") {
          date = text;
        }

        // 金额列：奖池在前，总投注额在后
        if (text.length > 3 && isDigitsOrCommas(text)) {
          if (pool === null) {
            pool = toInt(text);
          } else {
            sales = toInt(text);
          }
        }
      }

      if (reds.length === 6 && blue !== 0 && code !== "") {
        var redText = "" + reds[0];
        for (var k = 1; k < reds.length; k++) {
          redText = redText + "," + reds[k];
        }
        collected.push({
          code: code,
          date: date,
          red: redText,
          blue: String(blue),
          sales: sales,
          pool: pool,
        });
      }
    }
  }

  var collected = [];
  for (var year = startYear; year <= endYear; year++) {
    var yy = String(year).substring(2);
    var url = base + "?start=" + yy + "001&end=" + yy + "999";
    var html = http.get(url);
    if (html.length < 200) {
      continue;
    }
    var before = collected.length;
    parsePage(html, collected);
    log(year + " 年：" + (collected.length - before) + " 期");
  }

  log("500.com 合计 " + collected.length + " 期");
  return JSON.stringify(collected);
})();
