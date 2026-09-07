# 台灣科技大學 選課志願序小幫手

為了選到想要的課，你還在一門一門手動丟到課程查詢嗎？\
正在猶豫搶手的課程應該怎麼排比較好？

恭喜你，你找到好東西了\
這個小程式能自動分析你的選課清單\
並自動查詢選課人數與限制\
替你計算選課比例\
只需一鍵就能了解所有欲選課程的選課狀況

現在就下載體驗吧

> 有任何問題歡迎在 [Issues](https://github.com/NTUST-Tool/Course-order-assistant/issues) 提出，我有看到就會盡量回覆

### 下載連結：

| Windows | Linux | macOS Apple Silicon |
|:---:|:---:|:---:|
| [下載](https://github.com/NTUST-Tool/Course-order-assistant/releases/latest/download/Course-order-assistant.exe) | [下載](https://github.com/NTUST-Tool/Course-order-assistant/releases/latest/download/Course-order-assistant-linux.zip) | [下載](https://github.com/NTUST-Tool/Course-order-assistant/releases/latest/download/Course-order-assistant-macos-arm64.zip) |

其他平台與各版本提供的檔案，請查看[官方發布頁](https://github.com/NTUST-Tool/Course-order-assistant/releases/latest)，以該頁實際附件為準。若下載版本尚未提供主選單或監測功能，可參考[從原始碼執行](docs/monitoring.md#從原始碼執行)。

## 使用教學

以下為選課分析流程；課程監測請見[監測功能指南](docs/monitoring.md)。原有截圖為分析功能示意，啟動後請先在主選單選擇功能 `1`。

0. (非windows使用者) 解壓縮檔案
1. 打開選課清單

   ![image](https://github.com/NTUST-Tool/Course-order-assistant/assets/54392299/06ee9b2e-0bc5-46c4-a886-e62bfff529e1)

2. 按下ctrl+S，存檔類型建議選擇純 HTML

   ![image](https://github.com/NTUST-Tool/Course-order-assistant/assets/54392299/a48d1e87-0bbf-4ea0-a62a-a5e6aabbda7c)

3. 以下方式擇一啟動，再於主選單選擇功能 `1`：

   * a. 拖曳網頁存檔至Course-order-assistant.exe

     ![image](https://github.com/user-attachments/assets/a34b1f90-c540-4625-a19c-8d865f852d60)

   * b. 打開cmd，輸入 `Course-order-assistant.exe "XXX.html"`

     ![image](https://github.com/user-attachments/assets/ef413483-51b6-4120-bac5-4d2a1c16f488)

   * c. 不指定檔案直接啟動時，選擇功能 `1`，再輸入 HTML 檔案路徑。完成分析後，按 Enter 返回主選單；選擇 `0` 結束程式。

4. 根據查詢結果自行調整志願序

   ![image](https://github.com/user-attachments/assets/c5e72fe5-14e2-49a7-b876-14a718a6573b)

> 非官方開發，結果僅供參考，開發者 **不負** 任何責任

## 課程監測與通知

在主選單選擇功能 `2`，可定期查詢指定課程的人數；發現空缺時，會在本機提醒並透過 ntfy.sh 發送通知。支援多個課程代碼、自訂查詢間隔，以及輸入 `Q` 再按 Enter 返回主選單。

第一次使用需在 ntfy app 訂閱程式顯示的隨機頻道。頻道名稱不包含學號，但不是加密或身分驗證，請勿公開。程式只提供查詢與提醒，不會自動選課，也不保證能選上。

完整步驟、通知規則、隱私限制與疑難排解，請見[監測功能指南](docs/monitoring.md)。

---
## FAQ

1. 若windows無法執行，並顯示以下畫面，請安裝[ Visual C++ 可轉散發套件](https://aka.ms/vs/17/release/vc_redist.x64.exe)

> ![image](https://github.com/NTUST-Tool/Course-order-assistant/assets/54392299/38e3993f-21f2-4d23-bb7b-dfee35d1e9d8)

2. Windows 已保護您的電腦

> 按照以下步驟即可正常執行
>
> ![image](https://github.com/user-attachments/assets/42cbe323-0081-4ae4-b115-57734a4fb6f8)
>
> ![image](https://github.com/user-attachments/assets/18d82ef8-04a5-410d-aabc-8014d50a942a)

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=NTUST-Tool/Course-order-assistant&type=Date)](https://www.star-history.com/#NTUST-Tool/Course-order-assistant&Date)
