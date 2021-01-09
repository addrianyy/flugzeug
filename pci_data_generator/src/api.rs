#[derive(Copy, Clone, Debug)]
pub struct PciDeviceData {
    pub vendor_id:   u16,
    pub device_id:   u16,
    pub vendor_name: &'static str,
    pub device_name: &'static str,
}

impl PciDeviceData {
    pub fn find(vendor_id: u16, device_id: u16) -> Option<Self> {
        let vendor_index =
            VENDORS.binary_search_by_key(&vendor_id, |(vendor_id, ..)| *vendor_id).ok()?;
        
        let vendor = VENDORS[vendor_index];

        let device_index =
            vendor.2.binary_search_by_key(&device_id, |(device_id, ..)| *device_id).ok()?;

        let device = vendor.2[device_index];

        Some(Self {
            vendor_id,
            device_id,
            vendor_name: vendor.1,
            device_name: device.1,
        })
    }
}
