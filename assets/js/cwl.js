// 中国福彩官网数据源（首选）
// 接口: GET https://www.cwl.gov.cn/cwl_admin/front/cwlkj/search/kjxx/findDrawNotice
// 说明: 该接口总量约 2000+ 期，最早只到 2013 年，2013 年以前由 500.com 脚本补齐。
//
// 宿主提供: http.get(url) -> string, log(message)
// 返回值: JSON 字符串数组

(function () {
  var pageSize = 500;
  var pageNo = 1;
  var pageCount = 1;
  var collected = [];
  var base =
    "https://www.cwl.gov.cn/cwl_admin/front/cwlkj/search/kjxx/findDrawNotice" +
    "?name=ssq&systemType=PC&pageSize=" +
    pageSize +
    "&pageNo=";

  do {
    var raw = http.get(base + pageNo);
    var payload = JSON.parse(raw);

    if (payload.state !== 0) {
      throw new Error("福彩官网返回 state=" + payload.state + " " + (payload.message || ""));
    }

    pageCount = payload.pageNum || 1;
    var rows = payload.result || [];

    for (var i = 0; i < rows.length; i++) {
      var row = rows[i];
      collected.push({
        code: String(row.code),
        date: String(row.date || ""),
        red: String(row.red || ""),
        blue: String(row.blue || ""),
        sales: row.sales === undefined ? null : row.sales,
        pool: row.poolmoney === undefined ? null : row.poolmoney,
      });
    }

    log("福彩官网 第 " + pageNo + "/" + pageCount + " 页，累计 " + collected.length + " 期");
    pageNo = pageNo + 1;
  } while (pageNo <= pageCount && pageNo <= 60);

  return JSON.stringify(collected);
})();
