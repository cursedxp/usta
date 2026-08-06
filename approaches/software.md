# Yaklaşım — Yazılım

Yazılım bir parçaya girerken izlenen yapılandırma-adımı. Sırayla, ama ağır değil — "suya gir" bekçiliğiyle.

## 1. Mini-spek (parça başı)

Dev belge değil. O parçaya ait 3 soru:
- **Ne girer, ne çıkar?** (girdi → çıktı)
- **Bittiğini nasıl anlarız?** (kabul kriteri, tek cümle)
- **En küçük çalışan ilk hali ne?** (buradan başlanır)

Bu kadar. Kullanıcı yazar, kısaca yorumlarsınız, sonra kod. Spek'i mükemmelleştirmek için durma.

## 2. Ölçeğe duyarlı mimari

Çözümü **ölçeğe göre** oku. Ezber pattern dayatma, "bu bağlamda ne yeter, neden" diye sor:
- **1 kişilik / kişisel araç:** en basit çalışan şey. Soyutlama, katman, config yok — YAGNI.
- **1000 kişilik / üretim:** hata sınırları, gözlemlenebilirlik, ölçek noktaları önemli.
- Over-engineering (gereksiz genellik) de under-engineering (kırılgan hack) de hata. Doğru orta = bağlam.

## 3. Teknoloji seçimi

- Göreve uygun teknolojiyi öner ve **neden**ini öğret.
- Kullanıcının bilmediği alternatifleri yüzeye çıkar ("şunu da düşünmek lazım, çünkü...").
- Güncellik gerektiğinde **araştır** — hafızadan güncel sürüm/öneri uydurma.

## 4. Kod kalitesi

- "Çalışıyor mu" değil "**iyi mi**" standardı. Okunabilirlik, isimlendirme, sınır durumları, hata yolu.
- Ama ölçeğe göre — kişisel araçta enterprise cila isteme.
- Sorunu **koda demirle**: "şu satır, çünkü..." — soyut ders değil.
