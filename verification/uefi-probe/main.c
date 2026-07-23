/*
 * SanjuOS UEFI ABI verification probe.
 *
 * This is not product kernel code. It is a dependency-free PE32+ EFI
 * application used to validate the UEFI entry ABI, memory-map handoff,
 * ExitBootServices retry rule, serial output, and QEMU debug-exit contract
 * when the Rust compiler is unavailable in a constrained build environment.
 */

typedef unsigned char      UINT8;
typedef unsigned short     UINT16;
typedef unsigned int       UINT32;
typedef unsigned long long UINT64;
typedef unsigned long long UINTN;
typedef void              *EFI_HANDLE;
typedef UINTN              EFI_STATUS;
typedef UINT16             CHAR16;
typedef UINT64             EFI_PHYSICAL_ADDRESS;

#define EFIAPI __attribute__((ms_abi))
#define EFI_SUCCESS ((EFI_STATUS)0)
#define EFI_ERROR_BIT ((EFI_STATUS)1ULL << 63)
#define EFI_INVALID_PARAMETER (EFI_ERROR_BIT | 2)
#define EFI_BUFFER_TOO_SMALL  (EFI_ERROR_BIT | 5)
#define EFI_ERROR(status) (((status) & EFI_ERROR_BIT) != 0)

#define EFI_SYSTEM_TABLE_SIGNATURE 0x5453595320494249ULL
#define EFI_BOOT_SERVICES_SIGNATURE 0x56524553544f4f42ULL
#define MEMORY_MAP_CAPACITY (256U * 1024U)
#define EXIT_RETRIES 8U

#define COM1 0x03f8
#define COM1_THR 0
#define COM1_IER 1
#define COM1_FCR 2
#define COM1_LCR 3
#define COM1_MCR 4
#define COM1_LSR 5
#define COM1_TX_READY 0x20
#define QEMU_DEBUG_PORT 0x00e9
#define QEMU_EXIT_PORT  0x00f4
#define QEMU_EXIT_SUCCESS 0x10
#define QEMU_EXIT_FAILURE 0x11

typedef struct {
    UINT64 Signature;
    UINT32 Revision;
    UINT32 HeaderSize;
    UINT32 CRC32;
    UINT32 Reserved;
} EFI_TABLE_HEADER;

struct SIMPLE_TEXT_OUTPUT_PROTOCOL;
typedef EFI_STATUS (EFIAPI *EFI_TEXT_RESET)(struct SIMPLE_TEXT_OUTPUT_PROTOCOL *, UINT8);
typedef EFI_STATUS (EFIAPI *EFI_TEXT_STRING)(struct SIMPLE_TEXT_OUTPUT_PROTOCOL *, const CHAR16 *);
typedef EFI_STATUS (EFIAPI *EFI_TEXT_CLEAR)(struct SIMPLE_TEXT_OUTPUT_PROTOCOL *);

typedef struct SIMPLE_TEXT_OUTPUT_PROTOCOL {
    EFI_TEXT_RESET Reset;
    EFI_TEXT_STRING OutputString;
    void *TestString;
    void *QueryMode;
    void *SetMode;
    void *SetAttribute;
    EFI_TEXT_CLEAR ClearScreen;
    void *SetCursorPosition;
    void *EnableCursor;
    void *Mode;
} SIMPLE_TEXT_OUTPUT_PROTOCOL;

typedef struct {
    UINT32 Type;
    UINT32 Padding;
    UINT64 PhysicalStart;
    UINT64 VirtualStart;
    UINT64 NumberOfPages;
    UINT64 Attribute;
} EFI_MEMORY_DESCRIPTOR;

typedef EFI_STATUS (EFIAPI *EFI_GET_MEMORY_MAP)(
    UINTN *, EFI_MEMORY_DESCRIPTOR *, UINTN *, UINTN *, UINT32 *);
typedef EFI_STATUS (EFIAPI *EFI_EXIT_BOOT_SERVICES)(EFI_HANDLE, UINTN);

typedef struct {
    EFI_TABLE_HEADER Hdr;
    void *RaiseTPL;
    void *RestoreTPL;
    void *AllocatePages;
    void *FreePages;
    EFI_GET_MEMORY_MAP GetMemoryMap;
    void *AllocatePool;
    void *FreePool;
    void *CreateEvent;
    void *SetTimer;
    void *WaitForEvent;
    void *SignalEvent;
    void *CloseEvent;
    void *CheckEvent;
    void *InstallProtocolInterface;
    void *ReinstallProtocolInterface;
    void *UninstallProtocolInterface;
    void *HandleProtocol;
    void *Reserved;
    void *RegisterProtocolNotify;
    void *LocateHandle;
    void *LocateDevicePath;
    void *InstallConfigurationTable;
    void *LoadImage;
    void *StartImage;
    void *Exit;
    void *UnloadImage;
    EFI_EXIT_BOOT_SERVICES ExitBootServices;
} EFI_BOOT_SERVICES;

typedef struct {
    EFI_TABLE_HEADER Hdr;
    CHAR16 *FirmwareVendor;
    UINT32 FirmwareRevision;
    EFI_HANDLE ConsoleInHandle;
    void *ConIn;
    EFI_HANDLE ConsoleOutHandle;
    SIMPLE_TEXT_OUTPUT_PROTOCOL *ConOut;
    EFI_HANDLE StandardErrorHandle;
    SIMPLE_TEXT_OUTPUT_PROTOCOL *StdErr;
    void *RuntimeServices;
    EFI_BOOT_SERVICES *BootServices;
    UINTN NumberOfTableEntries;
    void *ConfigurationTable;
} EFI_SYSTEM_TABLE;

static UINT8 memory_map[MEMORY_MAP_CAPACITY] __attribute__((aligned(16)));

static inline void out8(UINT16 port, UINT8 value) {
    __asm__ volatile("outb %0, %1" : : "a"(value), "Nd"(port));
}

static inline UINT8 in8(UINT16 port) {
    UINT8 value;
    __asm__ volatile("inb %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

static inline void out32(UINT16 port, UINT32 value) {
    __asm__ volatile("outl %0, %1" : : "a"(value), "Nd"(port));
}

static void serial_init(void) {
    out8(COM1 + COM1_IER, 0x00);
    out8(COM1 + COM1_LCR, 0x80);
    out8(COM1 + 0, 0x01);
    out8(COM1 + 1, 0x00);
    out8(COM1 + COM1_LCR, 0x03);
    out8(COM1 + COM1_FCR, 0xc7);
    out8(COM1 + COM1_MCR, 0x0b);
}

static void serial_byte(UINT8 byte) {
    UINTN i;
    for (i = 0; i < 100000; ++i) {
        if ((in8(COM1 + COM1_LSR) & COM1_TX_READY) != 0) {
            break;
        }
    }
    out8(COM1 + COM1_THR, byte);
    out8(QEMU_DEBUG_PORT, byte);
}

static void serial_text(const char *text) {
    while (*text != 0) {
        serial_byte((UINT8)*text++);
    }
}

static void serial_uint(UINTN value) {
    char digits[32];
    UINTN cursor = sizeof(digits);
    if (value == 0) {
        serial_byte('0');
        return;
    }
    while (value != 0 && cursor != 0) {
        digits[--cursor] = (char)('0' + (value % 10));
        value /= 10;
    }
    while (cursor < sizeof(digits)) {
        serial_byte((UINT8)digits[cursor++]);
    }
}

static void firmware_text(SIMPLE_TEXT_OUTPUT_PROTOCOL *console, const char *text) {
    CHAR16 unit[2];
    if (console == (void *)0 || console->OutputString == (void *)0) {
        return;
    }
    unit[1] = 0;
    while (*text != 0) {
        unit[0] = (CHAR16)(UINT8)*text++;
        console->OutputString(console, unit);
    }
}

static __attribute__((noreturn)) void halt_forever(void) {
    for (;;) {
        __asm__ volatile("cli; hlt");
    }
}

EFI_STATUS EFIAPI efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *system_table) {
    EFI_BOOT_SERVICES *boot;
    SIMPLE_TEXT_OUTPUT_PROTOCOL *console;
    EFI_STATUS status = EFI_INVALID_PARAMETER;
    UINTN map_size = 0;
    UINTN map_key = 0;
    UINTN descriptor_size = 0;
    UINT32 descriptor_version = 0;
    UINTN attempt;

    serial_init();

    if (system_table == (void *)0 ||
        system_table->Hdr.Signature != EFI_SYSTEM_TABLE_SIGNATURE ||
        system_table->BootServices == (void *)0 ||
        system_table->BootServices->Hdr.Signature != EFI_BOOT_SERVICES_SIGNATURE) {
        serial_text("FATAL: invalid UEFI tables\r\n");
        out32(QEMU_EXIT_PORT, QEMU_EXIT_FAILURE);
        halt_forever();
    }

    boot = system_table->BootServices;
    console = system_table->ConOut;
    if (console != (void *)0 && console->ClearScreen != (void *)0) {
        console->ClearScreen(console);
    }
    firmware_text(console, "SanjuOS LLVM UEFI verification probe\r\n");
    firmware_text(console, "Capturing memory map and exiting firmware...\r\n");
    serial_text("SanjuOS LLVM UEFI verification probe\r\n");

    for (attempt = 0; attempt < EXIT_RETRIES; ++attempt) {
        map_size = MEMORY_MAP_CAPACITY;
        map_key = 0;
        descriptor_size = 0;
        descriptor_version = 0;

        status = boot->GetMemoryMap(
            &map_size,
            (EFI_MEMORY_DESCRIPTOR *)(void *)memory_map,
            &map_key,
            &descriptor_size,
            &descriptor_version);

        if (status == EFI_BUFFER_TOO_SMALL || EFI_ERROR(status) ||
            descriptor_size < sizeof(EFI_MEMORY_DESCRIPTOR) ||
            descriptor_size == 0 || map_size > MEMORY_MAP_CAPACITY ||
            (map_size % descriptor_size) != 0) {
            serial_text("FATAL: invalid UEFI memory map\r\n");
            out32(QEMU_EXIT_PORT, QEMU_EXIT_FAILURE);
            halt_forever();
        }

        status = boot->ExitBootServices(image, map_key);
        if (status == EFI_SUCCESS) {
            break;
        }
        if (status != EFI_INVALID_PARAMETER) {
            serial_text("FATAL: ExitBootServices failed\r\n");
            out32(QEMU_EXIT_PORT, QEMU_EXIT_FAILURE);
            halt_forever();
        }
    }

    if (status != EFI_SUCCESS) {
        serial_text("FATAL: ExitBootServices retry limit reached\r\n");
        out32(QEMU_EXIT_PORT, QEMU_EXIT_FAILURE);
        halt_forever();
    }

    serial_text("SanjuOS\r\n");
    serial_text("Milestone M1 probe: firmware exit and kernel ownership.\r\n");
    serial_text("Firmware boot services: exited\r\n");
    serial_text("Memory descriptors: ");
    serial_uint(map_size / descriptor_size);
    serial_text("\r\nKernel ownership gate: passed\r\n");

    out32(QEMU_EXIT_PORT, QEMU_EXIT_SUCCESS);
    halt_forever();
}
