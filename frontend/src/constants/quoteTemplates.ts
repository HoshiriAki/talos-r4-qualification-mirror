/**
 * 报价话术模板
 * 修改此处的文案后，重新构建前端即可生效。
 */

/** 报价话术后缀（追加在总价之后） */
export const QUOTE_SUFFIX = '包单程邮费，顺丰发货，配件包含机器本体及收纳包。'

/**
 * 生成完整报价文案
 * @param totalPrice 总价（元）
 */
export function generateQuoteText(totalPrice: number): string {
  return `总价是${totalPrice.toFixed(2)}元。${QUOTE_SUFFIX}`
}
