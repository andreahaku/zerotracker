/*
 * The APP page's instructions in every UI language (English, Italian, French,
 * German, Spanish, Japanese, Simplified Chinese), one paragraph per entry.
 */
#pragma once

#include <array>
#include <vector>

namespace screen {

// Same order as zt::i18n::Lang: En, It, Fr, De, Es, Ja, Zh.
inline const std::array<std::vector<const char*>, 7> kHelpText{{
    // en
    {
        "PAGES: key 4 goes to the next page, held to the one before: TIME, MIX, INST, SONG, APP.",
        "MOVE with the arrows (F X Z C). ENTER opens or edits the field under the cursor: a chain, "
        "phrase or instrument, a name, a value. ESC cancels, then goes back one level. ESC held "
        "3 s quits.",
        "EDIT A VALUE: ENTER or EDIT (5), then UP/DOWN for big steps, LEFT/RIGHT by one. ENTER "
        "keeps it, ESC undoes it. L held with the arrows edits directly.",
        "KEYS 5-8 are the actions named in the bottom bar; they change with the page.",
        "SELECT (7), the arrows grow the selection, then COPY or PASTE. Ctrl+C and Ctrl+V too.",
        "DEL clears a field. Ctrl+Z undo, Ctrl+Y redo, Ctrl+S save.",
        "SPACE plays and stops from the cursor.",
        "INSTRUMENT: LOAD and SAVE .m8i files; ENTER on the sample picks a .wav. In LOAD, PLAY "
        "previews songs, instruments and samples.",
        "MODS: scroll INSTRUMENT down past REV. Four modulators: type, DEST, then the type's "
        "own values. A new type starts from the M8's values.",
        "INST: below the eight tracks, every instrument of the song (POOL). LEFT/RIGHT pick "
        "NAME, DRY, MFX, DEL, REV or EQ; ENTER on NAME opens the instrument, on a value edits it "
        "(UP/DOWN by 16, LEFT/RIGHT by 1; ENTER keeps, ESC undoes).",
        "TRACKS: on INST the arrows move on the eight cards; keys 6 and 7 MUTE and SOLO the "
        "track, as on MIX. On SONG, LEFT/RIGHT change BPM and TRANSPOSE. On an instrument, "
        "PLAY plays it alone while the song is stopped. FX commands: EDIT+UP on an FX opens "
        "their grid.",
        "EQ: ENTER on an instrument's EQ opens its curve (an instrument without one gets its own). "
        "1 2 3 or ENTER pick a band; LEFT/RIGHT frequency, UP/DOWN gain (held: faster), - and = "
        "Q; keys 5-8 TYPE, MODE, RESET, PLAY; ESC goes back.",
        "HOME: RESUME, LOAD, NEW on keys 6 7 8. Songs live in ~/ZeroTracker.",
        "SAMPLE EDITOR (ENTER on EDIT next to a SAMPLER's sample): LEFT/RIGHT move the marker "
        "(L held: big steps), UP/DOWN zoom, ENTER switches start/end, SPACE listens. SLICE mode: "
        "LEFT/RIGHT choose a slice, S splits it, DEL joins it to the one before, ENTER edits it "
        "(the arrows move START or END, ENTER switches, ESC is done). 5 MODE: selection, loop, "
        "slices; 6 ACTION: normalize, crop, fades, reverse, SLICE EVEN in N parts, AUTO...; 8 "
        "SAVE (the same name overwrites, a new one makes a copy). Ctrl+Z / Y undo and redo.",
        "Fn+A / S / D: mute, volume down, volume up.",
    },
    // it
    {
        "PAGINE: il tasto 4 va alla pagina successiva, tenuto premuto a quella prima: TIME, MIX, "
        "INST, SONG, APP.",
        "MUOVITI con le frecce (F X Z C). ENTER apre o modifica il campo sotto il cursore: una "
        "chain, una phrase o uno strumento, un nome, un valore. ESC annulla, poi torna indietro "
        "di un livello. ESC tenuto 3 s esce.",
        "MODIFICARE UN VALORE: ENTER o EDIT (5), poi SU/GIU per salti grandi, SINISTRA/DESTRA di "
        "uno. ENTER lo tiene, ESC lo annulla. L tenuto con le frecce modifica subito.",
        "I TASTI 5-8 sono le azioni scritte nella barra in basso; cambiano con la pagina.",
        "SELECT (7), le frecce allargano la selezione, poi COPY o PASTE. Anche Ctrl+C e Ctrl+V.",
        "DEL cancella un campo. Ctrl+Z annulla, Ctrl+Y ripete, Ctrl+S salva.",
        "SPAZIO suona e ferma dal cursore.",
        "INSTRUMENT: LOAD e SAVE dei file .m8i; ENTER sul sample sceglie un .wav. Nel LOAD, PLAY "
        "fa sentire canzoni, strumenti e sample.",
        "MODS: scorri INSTRUMENT oltre REV. Quattro modulatori: tipo, DEST, poi i valori del "
        "tipo. Un tipo nuovo parte dai valori dell'M8.",
        "INST: sotto le otto tracce, tutti gli strumenti della canzone (POOL). SINISTRA/DESTRA "
        "scelgono NAME, DRY, MFX, DEL, REV o EQ; ENTER su NAME apre lo strumento, su un valore lo "
        "modifica (SU/GIU di 16, SINISTRA/DESTRA di 1; ENTER tiene, ESC annulla).",
        "TRACCE: in INST le frecce si muovono sulle otto card; i tasti 6 e 7 fanno MUTE e SOLO "
        "della traccia, come in MIX. In SONG, SINISTRA/DESTRA cambiano BPM e TRANSPOSE. Su uno "
        "strumento, PLAY lo fa ascoltare da solo a canzone ferma. Comandi FX: EDIT+SU su un FX "
        "apre la loro griglia.",
        "EQ: ENTER sull'EQ di uno strumento apre la sua curva (se non ne ha uno, riceve il suo). "
        "1 2 3 o ENTER scelgono la banda; SINISTRA/DESTRA frequenza, SU/GIU guadagno (tenuti: "
        "piu' veloci), - e = il Q; tasti 5-8 TIPO, MODO, AZZERA, SUONA; ESC torna indietro.",
        "HOME: RESUME, LOAD, NEW sui tasti 6 7 8. Le canzoni stanno in ~/ZeroTracker.",
        "EDITOR DEI SAMPLE (ENTER su EDIT accanto al sample di un SAMPLER): SINISTRA/DESTRA "
        "spostano il marcatore (L tenuto: passi grandi), SU/GIU zoomano, ENTER cambia inizio/fine, "
        "SPAZIO ascolta. Modo SLICE: SINISTRA/DESTRA scelgono la slice, S la divide, DEL la unisce "
        "alla precedente, ENTER la modifica (le frecce spostano INIZIO o FINE, ENTER cambia, ESC "
        "conferma). "
        "5 MODO: selezione, loop, slice; 6 AZIONI: normalizza, taglia, dissolvenze, inverti, "
        "slice...; 8 SALVA (stesso nome sovrascrive, un nome nuovo fa una copia). Ctrl+Z / Y "
        "annulla e ripete.",
        "Fn+A / S / D: muto, volume giu, volume su.",
    },
    // fr
    {
        "PAGES : la touche 4 passe à la page suivante, maintenue à la précédente : TIME, MIX, "
        "INST, SONG, APP.",
        "DÉPLACEZ-VOUS avec les flèches (F X Z C). ENTER ouvre ou modifie le champ sous le "
        "curseur : une chain, une phrase ou un instrument, un nom, une valeur. ESC annule, puis "
        "remonte d'un niveau. ESC maintenu 3 s quitte.",
        "MODIFIER UNE VALEUR : ENTER ou EDIT (5), puis HAUT/BAS par grands pas, GAUCHE/DROITE "
        "d'une unité. ENTER la garde, ESC l'annule. L maintenu avec les flèches modifie "
        "directement.",
        "LES TOUCHES 5-8 sont les actions indiquées dans la barre du bas ; elles changent selon "
        "la page.",
        "SELECT (7), les flèches étendent la sélection, puis COPY ou PASTE. Ctrl+C et Ctrl+V "
        "aussi.",
        "DEL efface un champ. Ctrl+Z annule, Ctrl+Y rétablit, Ctrl+S enregistre.",
        "SPACE lance et arrête la lecture depuis le curseur.",
        "INSTRUMENT : LOAD et SAVE des fichiers .m8i ; ENTER sur le sample choisit un .wav. Dans "
        "LOAD, PLAY fait écouter morceaux, instruments et samples.",
        "MODS : faites défiler INSTRUMENT au-delà de REV. Quatre modulateurs : type, DEST, puis "
        "les valeurs propres au type. Un nouveau type part des valeurs du M8.",
        "INST : sous les huit pistes, tous les instruments du morceau (POOL). GAUCHE/DROITE "
        "choisissent NAME, DRY, MFX, DEL, REV ou EQ ; ENTER sur NAME ouvre l'instrument, sur une "
        "valeur la modifie (HAUT/BAS par 16, GAUCHE/DROITE par 1 ; ENTER garde, ESC annule).",
        "PISTES : sur INST les flèches parcourent les huit cartes ; les touches 6 et 7 font MUTE "
        "et SOLO de la piste, comme sur MIX. Sur SONG, GAUCHE/DROITE changent BPM et TRANSPOSE. "
        "Sur un instrument, PLAY le joue seul quand le morceau est arrêté. Commandes FX : "
        "EDIT+HAUT sur un FX ouvre leur grille.",
        "EQ : ENTER sur l'EQ d'un instrument ouvre sa courbe (un instrument qui n'en a pas reçoit "
        "le sien). 1 2 3 ou ENTER choisissent une bande ; GAUCHE/DROITE fréquence, HAUT/BAS gain "
        "(maintenus : plus vite), - et = Q ; touches 5-8 TYPE, MODE, RESET, PLAY ; ESC revient en "
        "arrière.",
        "HOME : RESUME, LOAD, NEW sur les touches 6 7 8. Les morceaux sont dans ~/ZeroTracker.",
        "ÉDITEUR DE SAMPLE (ENTER sur EDIT à côté du sample d'un SAMPLER) : GAUCHE/DROITE "
        "déplacent le marqueur (L maintenu : grands pas), HAUT/BAS zooment, ENTER bascule "
        "début/fin, SPACE écoute. Mode SLICE : GAUCHE/DROITE choisissent une slice, S la coupe, "
        "DEL la fusionne avec la précédente, ENTER la modifie (les flèches déplacent START ou END, "
        "ENTER bascule, ESC termine). 5 MODE : sélection, boucle, slices ; 6 TRAIT. : "
        "normaliser, rogner, fondus, inverser, SLICE EVEN en N parties, AUTO... ; 8 SAVE (le même "
        "nom écrase, un nouveau nom crée une copie). Ctrl+Z / Y annulent et rétablissent.",
        "Fn+A / S / D : muet, volume moins, volume plus.",
    },
    // de
    {
        "SEITEN: Taste 4 geht zur nächsten Seite, gehalten zur vorherigen: TIME, MIX, INST, SONG, "
        "APP.",
        "BEWEGEN mit den Pfeilen (F X Z C). ENTER öffnet oder bearbeitet das Feld unter dem "
        "Cursor: eine Chain, eine Phrase oder ein Instrument, einen Namen, einen Wert. ESC bricht "
        "ab, dann geht es eine Ebene zurück. ESC 3 s gehalten beendet.",
        "WERT BEARBEITEN: ENTER oder EDIT (5), dann HOCH/RUNTER in großen Schritten, LINKS/RECHTS "
        "um eins. ENTER übernimmt, ESC verwirft. L gehalten mit den Pfeilen bearbeitet direkt.",
        "TASTEN 5-8 sind die Aktionen in der unteren Leiste; sie wechseln mit der Seite.",
        "SELECT (7), die Pfeile erweitern die Auswahl, dann COPY oder PASTE. Auch Ctrl+C und "
        "Ctrl+V.",
        "DEL leert ein Feld. Ctrl+Z rückgängig, Ctrl+Y wiederholen, Ctrl+S speichern.",
        "SPACE spielt und stoppt ab dem Cursor.",
        "INSTRUMENT: LOAD und SAVE von .m8i-Dateien; ENTER auf dem Sample wählt eine .wav. In "
        "LOAD hört PLAY Songs, Instrumente und Samples vor.",
        "MODS: INSTRUMENT über REV hinaus nach unten scrollen. Vier Modulatoren: Typ, DEST, dann "
        "die eigenen Werte des Typs. Ein neuer Typ startet mit den Werten des M8.",
        "INST: unter den acht Spuren alle Instrumente des Songs (POOL). LINKS/RECHTS wählen NAME, "
        "DRY, MFX, DEL, REV oder EQ; ENTER auf NAME öffnet das Instrument, auf einem Wert "
        "bearbeitet es ihn (HOCH/RUNTER um 16, LINKS/RECHTS um 1; ENTER übernimmt, ESC "
        "verwirft).",
        "SPUREN: auf INST bewegen die Pfeile über die acht Karten; Tasten 6 und 7 schalten MUTE "
        "und SOLO der Spur, wie auf MIX. Auf SONG ändern LINKS/RECHTS BPM und TRANSPOSE. Auf "
        "einem Instrument spielt PLAY es allein, wenn der Song steht. FX-Befehle: EDIT+HOCH auf "
        "einem FX öffnet ihr Raster.",
        "EQ: ENTER auf dem EQ eines Instruments öffnet seine Kurve (ein Instrument ohne EQ "
        "bekommt einen eigenen). 1 2 3 oder ENTER wählen ein Band; LINKS/RECHTS Frequenz, "
        "HOCH/RUNTER Gain (gehalten: schneller), - und = Q; Tasten 5-8 TYPE, MODE, RESET, PLAY; "
        "ESC geht zurück.",
        "HOME: RESUME, LOAD, NEW auf den Tasten 6 7 8. Songs liegen in ~/ZeroTracker.",
        "SAMPLE-EDITOR (ENTER auf EDIT neben dem Sample eines SAMPLER): LINKS/RECHTS bewegen den "
        "Marker (L gehalten: große Schritte), HOCH/RUNTER zoomen, ENTER wechselt Start/Ende, "
        "SPACE hört ab. SLICE-Modus: LINKS/RECHTS wählen eine Slice, S teilt sie, DEL verbindet "
        "sie mit der vorherigen, ENTER bearbeitet sie (die Pfeile bewegen START oder END, ENTER "
        "wechselt, ESC beendet). 5 MODE: Auswahl, Loop, Slices; 6 BEARB.: normalisieren, "
        "zuschneiden, Fades, umkehren, SLICE EVEN in N Teile, AUTO...; 8 SAVE (derselbe Name "
        "überschreibt, ein neuer erstellt eine Kopie). Ctrl+Z / Y rückgängig und wiederholen.",
        "Fn+A / S / D: stumm, leiser, lauter.",
    },
    // es
    {
        "PÁGINAS: la tecla 4 va a la página siguiente, mantenida a la anterior: TIME, MIX, INST, "
        "SONG, APP.",
        "MUÉVETE con las flechas (F X Z C). ENTER abre o edita el campo bajo el cursor: una "
        "chain, una phrase o un instrumento, un nombre, un valor. ESC cancela y luego sube un "
        "nivel. ESC mantenida 3 s sale.",
        "EDITAR UN VALOR: ENTER o EDIT (5), luego ARRIBA/ABAJO en pasos grandes, "
        "IZQUIERDA/DERECHA de uno en uno. ENTER lo conserva, ESC lo deshace. L mantenida con las "
        "flechas edita directamente.",
        "LAS TECLAS 5-8 son las acciones de la barra inferior; cambian con la página.",
        "SELECT (7), las flechas amplían la selección, luego COPY o PASTE. También Ctrl+C y "
        "Ctrl+V.",
        "DEL borra un campo. Ctrl+Z deshace, Ctrl+Y rehace, Ctrl+S guarda.",
        "SPACE reproduce y detiene desde el cursor.",
        "INSTRUMENT: LOAD y SAVE de archivos .m8i; ENTER sobre el sample elige un .wav. En LOAD, "
        "PLAY previsualiza canciones, instrumentos y samples.",
        "MODS: desplaza INSTRUMENT hacia abajo más allá de REV. Cuatro moduladores: tipo, DEST y "
        "luego los valores propios del tipo. Un tipo nuevo parte de los valores del M8.",
        "INST: bajo las ocho pistas, todos los instrumentos de la canción (POOL). "
        "IZQUIERDA/DERECHA eligen NAME, DRY, MFX, DEL, REV o EQ; ENTER sobre NAME abre el "
        "instrumento, sobre un valor lo edita (ARRIBA/ABAJO de 16 en 16, IZQUIERDA/DERECHA de 1 "
        "en 1; ENTER conserva, ESC deshace).",
        "PISTAS: en INST las flechas recorren las ocho tarjetas; las teclas 6 y 7 hacen MUTE y "
        "SOLO de la pista, como en MIX. En SONG, IZQUIERDA/DERECHA cambian BPM y TRANSPOSE. En "
        "un instrumento, PLAY lo toca solo con la canción parada. Comandos FX: EDIT+ARRIBA sobre "
        "un FX abre su cuadrícula.",
        "EQ: ENTER sobre el EQ de un instrumento abre su curva (un instrumento sin EQ recibe uno "
        "propio). 1 2 3 o ENTER eligen una banda; IZQUIERDA/DERECHA frecuencia, ARRIBA/ABAJO "
        "ganancia (mantenidas: más rápido), - y = Q; teclas 5-8 TYPE, MODE, RESET, PLAY; ESC "
        "vuelve atrás.",
        "HOME: RESUME, LOAD, NEW en las teclas 6 7 8. Las canciones están en ~/ZeroTracker.",
        "EDITOR DE SAMPLES (ENTER sobre EDIT junto al sample de un SAMPLER): IZQUIERDA/DERECHA "
        "mueven el marcador (L mantenida: pasos grandes), ARRIBA/ABAJO hacen zoom, ENTER alterna "
        "inicio/fin, SPACE escucha. Modo SLICE: IZQUIERDA/DERECHA eligen una slice, S la divide, "
        "DEL la une a la anterior, ENTER la edita (las flechas mueven START o END, ENTER alterna, "
        "ESC termina). 5 MODE: selección, loop, slices; 6 PROC.: normalizar, recortar, "
        "fundidos, invertir, SLICE EVEN en N partes, AUTO...; 8 SAVE (el mismo nombre "
        "sobrescribe, uno nuevo crea una copia). Ctrl+Z / Y deshacen y rehacen.",
        "Fn+A / S / D: silencio, bajar volumen, subir volumen.",
    },
    // ja
    {
        "ページ：4キーで次のページ、長押しで前のページへ：TIME、MIX、INST、SONG、APP。",
        "矢印キー（F X Z C）で移動。ENTERでカーソル位置の項目（チェーン、フレーズ、"
        "インストゥルメント、名前、値）を開くか編集。ESCでキャンセル、次に1階層戻る。"
        "ESCを3秒長押しで終了。",
        "値の編集：ENTERまたはEDIT（5）を押し、上下で大きく、左右で1ずつ変更。"
        "ENTERで確定、ESCで取り消し。Lを押しながら矢印で直接編集。",
        "5-8キーは下部バーに表示されたアクション。ページごとに変わります。",
        "SELECT（7）を押し、矢印で選択範囲を広げてCOPYまたはPASTE。Ctrl+CとCtrl+Vも使えます。",
        "DELで項目を消去。Ctrl+Zで元に戻す、Ctrl+Yでやり直し、Ctrl+Sで保存。",
        "SPACEでカーソル位置から再生／停止。",
        "INSTRUMENT：.m8iファイルをLOAD／SAVE。サンプル上でENTERを押して.wavを選択。"
        "LOAD画面ではPLAYでソング、インストゥルメント、サンプルを試聴。",
        "MODS：INSTRUMENTをREVより下へスクロール。モジュレーターは4つ：タイプ、DEST、"
        "そのタイプ固有の値。新しいタイプはM8の値から始まります。",
        "INST：8トラックの下に、ソングの全インストゥルメント（POOL）。左右でNAME、DRY、MFX、"
        "DEL、REV、EQを選択。NAMEでENTERを押すとインストゥルメントを開き、値の上では編集"
        "（上下で16ずつ、左右で1ずつ。ENTERで確定、ESCで取り消し）。",
        "トラック：INSTでは矢印で8枚のカードを移動。6と7キーでトラックをMUTE／SOLO（MIXと同じ）。"
        "SONGでは左右でBPMとTRANSPOSEを変更。インストゥルメント上では、停止中にPLAYで単独試聴。"
        "FXコマンド：FX上でEDIT+上を押すと一覧グリッドを開く。",
        "EQ：インストゥルメントのEQでENTERを押すとカーブを開く（EQのないインストゥルメント"
        "には専用のEQを作成）。1 2 3またはENTERでバンドを選択。左右で周波数、上下でゲイン"
        "（長押しで高速）、-と=でQ。5-8キーはTYPE、MODE、RESET、PLAY。ESCで戻る。",
        "HOME：6 7 8キーでRESUME、LOAD、NEW。ソングは~/ZeroTrackerにあります。",
        "サンプルエディター（SAMPLERのサンプル横のEDITでENTER）：左右でマーカーを移動"
        "（L長押しで大きく）、上下でズーム、ENTERで開始／終了を切り替え、SPACEで試聴。"
        "SLICEモード：左右でスライスを選択、Sで分割、DELで前のスライスと結合、ENTERで編集"
        "（矢印でSTARTまたはENDを移動、ENTERで切り替え、ESCで完了）。5 MODE：選択範囲、"
        "ループ、スライス。6 処理：ノーマライズ、トリミング、フェード、リバース、"
        "SLICE EVENでN等分、AUTO...。8 SAVE（同じ名前で上書き、新しい名前でコピーを作成）。"
        "Ctrl+Z / Yで元に戻す／やり直し。",
        "Fn+A / S / D：ミュート、音量を下げる、音量を上げる。",
    },
    // zh
    {
        "页面：按 4 键进入下一页，长按返回上一页：TIME、MIX、INST、SONG、APP。",
        "用方向键（F X Z C）移动。ENTER 打开或编辑光标处的字段：chain、phrase 或乐器、名称、"
        "数值。ESC 先取消，再返回上一级。长按 ESC 3 秒退出。",
        "编辑数值：按 ENTER 或 EDIT（5），然后上/下大步调整，左/右逐一调整。ENTER 保留，"
        "ESC 撤销。按住 L 配合方向键可直接编辑。",
        "5-8 键是底栏所示的操作，随页面变化。",
        "SELECT（7），用方向键扩大选区，然后 COPY 或 PASTE。也可用 Ctrl+C 和 Ctrl+V。",
        "DEL 清空字段。Ctrl+Z 撤销，Ctrl+Y 重做，Ctrl+S 保存。",
        "SPACE 从光标处播放和停止。",
        "INSTRUMENT：LOAD 和 SAVE .m8i 文件；在采样上按 ENTER 选择 .wav。在 LOAD 中，"
        "PLAY 可试听歌曲、乐器和采样。",
        "MODS：将 INSTRUMENT 向下滚动越过 REV。四个调制器：类型、DEST，然后是该类型自己的"
        "数值。新类型从 M8 的数值开始。",
        "INST：八条音轨下方是歌曲的全部乐器（POOL）。左/右选择 NAME、DRY、MFX、DEL、REV 或 "
        "EQ；在 NAME 上按 ENTER 打开乐器，在数值上按 ENTER 编辑（上/下每次 16，左/右每次 1；"
        "ENTER 保留，ESC 撤销）。",
        "音轨：在 INST 中方向键在八张卡片间移动；6 和 7 键对音轨 MUTE 和 SOLO（与 MIX 相同）。"
        "在 SONG 中左/右改变 BPM 和 TRANSPOSE。在乐器上，停止时按 PLAY 单独试听。"
        "FX 命令：在 FX 上按 EDIT+上 打开命令网格。",
        "EQ：在乐器的 EQ 上按 ENTER 打开其曲线（没有 EQ 的乐器会获得自己的 EQ）。1 2 3 或 "
        "ENTER 选择频段；左/右调频率，上/下调增益（按住更快），- 和 = 调 Q；5-8 键为 TYPE、"
        "MODE、RESET、PLAY；ESC 返回。",
        "HOME：6 7 8 键为 RESUME、LOAD、NEW。歌曲存放在 ~/ZeroTracker。",
        "采样编辑器（在 SAMPLER 采样旁的 EDIT 上按 ENTER）：左/右移动标记（按住 L：大步），"
        "上/下缩放，ENTER 切换起点/终点，SPACE 试听。SLICE 模式：左/右选择切片，S 分割，"
        "DEL 与前一个合并，ENTER 编辑（方向键移动 START 或 END，ENTER 切换，ESC 完成）。"
        "5 MODE：选区、循环、切片；6 处理：标准化、裁剪、淡入淡出、反转、SLICE EVEN "
        "分成 N 份、AUTO...；8 SAVE（同名覆盖，新名称创建副本）。Ctrl+Z / Y 撤销和重做。",
        "Fn+A / S / D：静音、减小音量、增大音量。",
    },
}};

}  // namespace screen
