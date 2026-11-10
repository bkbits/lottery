# 实现一个双色球号码预测软件，

## 技术栈

- rust
- slint
- slint kit 组件库
- boa (js内嵌引擎，用于数据源获取脚本)
- sea-orm
- sqlite3

## 兼容平台

- windows
- linux
- mac
- android
- ios(可选)

## 数据源

- 500.com 初次历史数据获取接口以及备选接口
  example: GET https://datachart.500.com/ssq/history/newinc/history.php?start=03001&end=26001
- 中国福彩官网接口 首选接口
  example: GET https://www.cwl.gov.cn/cwl_admin/front/cwlkj/search/kjxx/findDrawNotice
  ?name=ssq&pageNo=1&pageSize=100&systemType=PC

## 要求包含功能：

- 往期双色球统计数据
  - 每个数字的遗漏值：当前遗漏，平均遗漏，最大遗漏
  - 当前冷热号
  - 连出循环
  - 和值
  - 跨度
  - 奇偶比
  - 大小比
  - 质合比
  - 012路比
  - 区间比
  - AC值
  - 平均值
  - 连号
  - 重号
  - 同尾号
  - 邻号
  - 尾数和
  - 其它（自由发挥）
- 预测下期号码
  - 预测数量
  - 预测算法（可以选多个，但是合理互斥，默认至少选择一个，每个算法给出一个问号图标，点击打开具体预测算法介绍）
  - 复制预测号码：以 01,02,03,04,05,06+08的方式每行一组复制，其中 +08是篮球
